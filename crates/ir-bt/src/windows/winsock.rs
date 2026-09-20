//! As chamadas ao Winsock `AF_BTH` e à API de Bluetooth do Win32.
//!
//! **Este é o único módulo do crate onde `unsafe` é permitido**
//! ([09, §4](../../../../docs/09-padroes-de-codigo.md)). Toda chamada de FFI é embrulhada numa
//! função segura aqui, e o resto do crate usa só o embrulho — nem `windows::Win32` aparece fora
//! deste arquivo.
//!
//! `AF_BTH` e não `WinRT`: as APIs `Windows.Devices.*` dependem de infraestrutura por usuário e
//! não são alvo suportado na sessão 0, onde este transporte precisa existir
//! ([ADR-0005](../../../../docs/adr/0005-bluetooth-rfcomm-winsock.md)).
//!
//! O preço é que estes sockets são **síncronos**: quem os transforma em algo que o `tokio` sabe
//! esperar é a [`ponte`](super::ponte).

#![allow(unsafe_code)]

use std::io;
use std::sync::Once;

// A família de sockets e o protocolo do Bluetooth ficam no módulo de **Bluetooth**, e não no de
// WinSock, junto com o `SOCKADDR_BTH` que os acompanha. Só as chamadas genéricas de socket é que
// vêm do WinSock.
use windows::Win32::Devices::Bluetooth::{
    AF_BTH, BLUETOOTH_DEVICE_INFO, BLUETOOTH_DEVICE_SEARCH_PARAMS, BLUETOOTH_FIND_RADIO_PARAMS,
    BTHPROTO_RFCOMM, BluetoothFindDeviceClose, BluetoothFindFirstDevice, BluetoothFindFirstRadio,
    BluetoothFindNextDevice, BluetoothFindRadioClose, SOCKADDR_BTH,
};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Networking::WinSock::{
    INVALID_SOCKET, SD_BOTH, SEND_RECV_FLAGS, SOCK_STREAM, SOCKADDR, SOCKET, SOCKET_ERROR, WSADATA,
    WSAGetLastError, WSAStartup, accept, bind, closesocket, connect, listen, recv, send, shutdown,
    socket,
};

use crate::addr::BdAddr;
use crate::radio::Dispositivo;

/// Um socket do Winsock.
///
/// Um `usize` embrulhado: `Send` e `Sync` por si, o que permite a thread de leitura e a de
/// escrita usarem o mesmo socket, como o Winsock admite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Sock(SOCKET);

/// Garante que o Winsock foi inicializado, uma vez por processo.
///
/// A biblioteca padrão faz isso ao tocar em `std::net`, e este caminho nunca toca. Sem a
/// inicialização, a primeira chamada falha com `WSANOTINITIALISED`.
pub(super) fn iniciar() {
    static UMA_VEZ: Once = Once::new();
    UMA_VEZ.call_once(|| {
        let mut dados = WSADATA::default();
        // SAFETY: `dados` é um `WSADATA` válido e exclusivo desta chamada; 2.2 é a versão que o
        // Windows suporta desde sempre. O valor de retorno é ignorado de propósito: se falhar,
        // a falha aparece na primeira chamada de socket, com o código de erro certo.
        let _ = unsafe { WSAStartup(0x0202, &raw mut dados) };
    });
}

/// O último erro do Winsock, como erro de E/S.
fn ultimo_erro() -> io::Error {
    // SAFETY: sem parâmetros e sem pré-condição; devolve o código de erro da thread corrente.
    let codigo = unsafe { WSAGetLastError() };
    io::Error::from_raw_os_error(codigo.0)
}

/// O tamanho de um endereço Bluetooth, no inteiro que a API pede.
///
/// A estrutura tem algumas dezenas de bytes; a conversão não tem como faltar, e a reserva existe
/// só para não haver um caminho de pânico neste módulo.
fn tamanho_do_endereco() -> i32 {
    i32::try_from(size_of::<SOCKADDR_BTH>()).unwrap_or(i32::MAX)
}

/// O endereço Bluetooth visto como o `SOCKADDR` genérico que o Winsock recebe.
///
/// **Esta conversão é o contrato da API, não uma escolha nossa.** `bind`, `connect` e `accept`
/// recebem sempre `*const SOCKADDR` e decidem como ler os bytes pelo campo `addressFamily`, que
/// é o primeiro campo nos dois tipos. O `SOCKADDR_BTH` declara alinhamento menor que o do
/// genérico, e é disso que o lint reclama; o sistema lê os campos pelo tipo verdadeiro, que ele
/// escolhe pela família, e nunca pelo genérico.
///
/// Concentrar as duas conversões aqui deixa essa justificativa num lugar só, em vez de espalhar
/// exceções pelos pontos de uso.
#[allow(clippy::cast_ptr_alignment)]
fn como_sockaddr(endereco: &SOCKADDR_BTH) -> *const SOCKADDR {
    std::ptr::from_ref(endereco).cast::<SOCKADDR>()
}

/// Como [`como_sockaddr`], para quando o sistema é quem escreve o endereço.
#[allow(clippy::cast_ptr_alignment)]
fn como_sockaddr_mut(endereco: &mut SOCKADDR_BTH) -> *mut SOCKADDR {
    std::ptr::from_mut(endereco).cast::<SOCKADDR>()
}

/// Abre um socket RFCOMM.
pub(super) fn abrir_socket() -> io::Result<Sock> {
    iniciar();
    // SAFETY: os três valores são constantes da própria API (família Bluetooth, fluxo, RFCOMM).
    let bruto = unsafe {
        socket(
            i32::from(AF_BTH),
            SOCK_STREAM,
            BTHPROTO_RFCOMM.cast_signed(),
        )
    }
    .map_err(|_| ultimo_erro())?;
    if bruto == INVALID_SOCKET {
        return Err(ultimo_erro());
    }
    Ok(Sock(bruto))
}

/// O endereço de um par no canal dado.
fn endereco(alvo: BdAddr, canal: u8) -> SOCKADDR_BTH {
    SOCKADDR_BTH {
        addressFamily: AF_BTH,
        btAddr: alvo.para_u64(),
        serviceClassId: windows::core::GUID::zeroed(),
        port: u32::from(canal),
    }
}

/// Conecta ao par, no canal do produto. Bloqueia.
pub(super) fn conectar(sock: Sock, alvo: BdAddr, canal: u8) -> io::Result<()> {
    let destino = endereco(alvo, canal);
    // SAFETY: `destino` vive até o fim da chamada, e o tamanho declarado é o do próprio tipo.
    let saida = unsafe { connect(sock.0, como_sockaddr(&destino), tamanho_do_endereco()) };
    if saida == SOCKET_ERROR {
        return Err(ultimo_erro());
    }
    Ok(())
}

/// Vincula ao canal do produto e passa a escutar.
pub(super) fn vincular_e_escutar(sock: Sock, canal: u8, fila: i32) -> io::Result<()> {
    let local = endereco(BdAddr::NULO, canal);
    // SAFETY: como em `conectar` — o endereço vive até o fim da chamada.
    let vinculo = unsafe { bind(sock.0, como_sockaddr(&local), tamanho_do_endereco()) };
    if vinculo == SOCKET_ERROR {
        return Err(ultimo_erro());
    }
    // SAFETY: socket vinculado; `fila` é o tamanho da fila de pendentes.
    if unsafe { listen(sock.0, fila) } == SOCKET_ERROR {
        return Err(ultimo_erro());
    }
    Ok(())
}

/// Espera alguém conectar. Bloqueia.
pub(super) fn aceitar(escuta: Sock) -> io::Result<(Sock, BdAddr)> {
    let mut origem = SOCKADDR_BTH::default();
    let mut tamanho = tamanho_do_endereco();
    // SAFETY: `origem` e `tamanho` são exclusivos desta chamada, e o tamanho declarado é o do
    // buffer de verdade — é ele que impede o sistema de escrever além do fim.
    let aceito = unsafe {
        accept(
            escuta.0,
            Some(como_sockaddr_mut(&mut origem)),
            Some(&raw mut tamanho),
        )
    }
    .map_err(|_| ultimo_erro())?;
    Ok((Sock(aceito), BdAddr::de_u64(origem.btAddr)))
}

/// Lê do socket. Bloqueia até chegar algo, o par fechar, ou dar erro.
///
/// Zero significa fim de fluxo.
pub(super) fn receber(sock: Sock, buffer: &mut [u8]) -> io::Result<usize> {
    // SAFETY: `buffer` é uma fatia válida e exclusiva; a API recebe o tamanho dela.
    let lidos = unsafe { recv(sock.0, buffer, SEND_RECV_FLAGS(0)) };
    if lidos == SOCKET_ERROR {
        return Err(ultimo_erro());
    }
    usize::try_from(lidos).map_err(|_| io::Error::other("tamanho negativo na leitura"))
}

/// Escreve no socket, até o último byte.
pub(super) fn enviar_tudo(sock: Sock, bytes: &[u8]) -> io::Result<()> {
    let mut restante = bytes;
    while !restante.is_empty() {
        // SAFETY: `restante` é uma fatia válida; a API recebe o tamanho dela.
        let escritos = unsafe { send(sock.0, restante, SEND_RECV_FLAGS(0)) };
        if escritos == SOCKET_ERROR {
            return Err(ultimo_erro());
        }
        let avanco = usize::try_from(escritos)
            .map_err(|_| io::Error::other("tamanho negativo na escrita"))?;
        if avanco == 0 {
            return Err(io::Error::from(io::ErrorKind::WriteZero));
        }
        restante = restante.get(avanco..).unwrap_or(&[]);
    }
    Ok(())
}

/// Desbloqueia quem estiver esperando neste socket.
///
/// Sem isto, a thread parada num `recv` só sairia quando o par falasse — e o encerramento do
/// serviço travaria esperando uma mensagem que não vem.
pub(super) fn encerrar(sock: Sock) {
    // SAFETY: encerrar um socket já encerrado devolve erro, que é ignorado de propósito.
    let _ = unsafe { shutdown(sock.0, SD_BOTH) };
}

/// Fecha o socket.
pub(super) fn fechar(sock: Sock) {
    // SAFETY: cada socket é fechado uma vez só — o dono é o guarda de [`super::ponte`].
    let _ = unsafe { closesocket(sock.0) };
}

/// O tamanho de uma estrutura, no inteiro sem sinal que a API pede em `dwSize`.
///
/// É assim que esta API sabe qual versão do registro está recebendo.
fn tamanho_de<T>() -> u32 {
    u32::try_from(size_of::<T>()).unwrap_or(u32::MAX)
}

/// Se há rádio Bluetooth utilizável nesta máquina.
pub(super) fn ha_radio() -> bool {
    iniciar();
    let parametros = BLUETOOTH_FIND_RADIO_PARAMS {
        dwSize: tamanho_de::<BLUETOOTH_FIND_RADIO_PARAMS>(),
    };
    let mut radio = HANDLE::default();
    // SAFETY: `dwSize` declara o tamanho da estrutura, como a API exige; `radio` é exclusivo.
    let busca = unsafe { BluetoothFindFirstRadio(&raw const parametros, &raw mut radio) };
    match busca {
        Ok(alca) => {
            // SAFETY: as duas alças vieram desta chamada e são fechadas uma vez só.
            unsafe {
                let _ = CloseHandle(radio);
                let _ = BluetoothFindRadioClose(alca);
            }
            true
        }
        Err(_) => false,
    }
}

/// Os computadores pareados neste sistema.
///
/// Não devolve `Result`, e a ausência é a informação: nenhum rádio e nenhum par pareado dão a
/// mesma resposta honesta — uma lista vazia. Prometer um fracasso que não acontece obrigaria
/// quem chama a tratar um caminho que nunca existe.
pub(super) fn pareados() -> Vec<Dispositivo> {
    iniciar();
    let busca = BLUETOOTH_DEVICE_SEARCH_PARAMS {
        dwSize: tamanho_de::<BLUETOOTH_DEVICE_SEARCH_PARAMS>(),
        fReturnAuthenticated: true.into(),
        fReturnRemembered: true.into(),
        // Sem inquérito: o produto não sai procurando aparelho novo pelo ar. Ele mostra quem já
        // foi pareado nas configurações do sistema (ADR-0005, Decisão B).
        fReturnUnknown: false.into(),
        fReturnConnected: true.into(),
        fIssueInquiry: false.into(),
        cTimeoutMultiplier: 0,
        hRadio: HANDLE::default(),
    };
    let mut info = BLUETOOTH_DEVICE_INFO {
        dwSize: tamanho_de::<BLUETOOTH_DEVICE_INFO>(),
        ..Default::default()
    };

    // SAFETY: as duas estruturas declaram o próprio tamanho em `dwSize`, que é como esta API
    // sabe qual versão do registro está recebendo, e ambas vivem por toda a busca.
    let Ok(alca) = (unsafe { BluetoothFindFirstDevice(&raw const busca, &raw mut info) }) else {
        return Vec::new(); // nenhum dispositivo é uma resposta, não um erro
    };

    let mut encontrados = Vec::new();
    loop {
        if info.fAuthenticated.as_bool() {
            encontrados.push(Dispositivo {
                endereco: BdAddr::de_u64(endereco_de(&info)),
                nome: nome_de(&info),
                conectado: info.fConnected.as_bool(),
                classe: info.ulClassofDevice,
            });
        }
        info.dwSize = tamanho_de::<BLUETOOTH_DEVICE_INFO>();
        // SAFETY: `alca` veio de `BluetoothFindFirstDevice` e continua aberta; `info` é exclusivo.
        if unsafe { BluetoothFindNextDevice(alca, &raw mut info) }.is_err() {
            break;
        }
    }
    // SAFETY: a alça é fechada uma vez só, aqui.
    let _ = unsafe { BluetoothFindDeviceClose(alca) };
    encontrados
}

/// O endereço do dispositivo, como inteiro de 64 bits.
fn endereco_de(info: &BLUETOOTH_DEVICE_INFO) -> u64 {
    // SAFETY: os dois campos da união ocupam o mesmo espaço e `ullLong` cobre todos os bytes
    // dela; ler o inteiro é sempre válido, qualquer que tenha sido o campo escrito.
    unsafe { info.Address.Anonymous.ullLong }
}

/// O nome do dispositivo, com o endereço como reserva.
fn nome_de(info: &BLUETOOTH_DEVICE_INFO) -> String {
    let fim = info
        .szName
        .iter()
        .position(|caractere| *caractere == 0)
        .unwrap_or(info.szName.len());
    let nome = info
        .szName
        .get(..fim)
        .map(String::from_utf16_lossy)
        .unwrap_or_default();
    if nome.trim().is_empty() {
        // Um nome em branco na lista não dá ao usuário como escolher.
        BdAddr::de_u64(endereco_de(info)).to_string()
    } else {
        nome
    }
}
