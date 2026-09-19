//! Descoberta na rede local: "quem está aí?" por broadcast, numa porta só do produto.
//!
//! Cada serviço escuta na [`PORTA_DA_DESCOBERTA`] e responde a uma [pergunta](pergunta) com o que
//! um candidato precisa — id da instalação, nome da máquina e a porta em que atende — e **nada
//! mais**: sem usuário, sem chave, sem código de pareamento, sem conteúdo de clipboard
//! ([03, §10](../../../docs/03-protocolo.md)). Quem procura pergunta a todas as sub-redes locais e
//! junta as respostas.
//!
//! # Por que não mDNS
//!
//! Foi mDNS até a bancada mostrar o serviço do Windows mudo. A porta 5353 é de todo mundo — Chrome,
//! o Quick Share, o próprio Windows —, e o Windows não deixa um processo de **outra conta** reusar
//! uma porta já aberta: o serviço, que roda como SYSTEM, ficava sem o socket IPv4, não anunciava e
//! não ouvia ninguém. Numa porta só nossa não há com quem disputar, e a regra de firewall que o
//! instalador já cria para o serviço a cobre.
//!
//! De quebra, a resposta vem do endereço que de fato alcança quem perguntou: com mDNS, o anúncio
//! trazia todos os endereços da máquina, inclusive os do WSL, do Hyper-V e do Docker, e era preciso
//! adivinhar qual discar.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use tokio::net::UdpSocket;

use crate::error::Result;

/// A porta em que todo serviço responde à pergunta. Vizinha da do produto, e fixa: quem procura
/// ainda não sabe nada sobre o outro lado, nem a porta dele.
pub const PORTA_DA_DESCOBERTA: u16 = 52524;

/// O começo de toda mensagem da descoberta: separa este protocolo de qualquer outro tráfego que
/// chegue à porta.
const MAGICA: &[u8; 6] = b"IRDESC";

/// A versão deste formato.
const VERSAO: u8 = 1;

/// Pergunta e resposta, no byte depois da mágica.
const PERGUNTA: u8 = b'?';
const RESPOSTA: u8 = b'!';

/// O maior nome que a resposta carrega. Nome de máquina é curto; o teto existe para uma resposta
/// forjada não fazer ninguém alocar à vontade.
const NOME_MAXIMO: usize = 63;

/// Um computador que a descoberta encontrou.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Como o usuário reconhece este computador (o nome anunciado).
    pub label: String,
    /// O identificador da instalação, como veio na resposta.
    pub machine: String,
    /// Onde alcançá-lo: o endereço de onde a resposta veio, na porta que ela anunciou.
    pub addr: SocketAddr,
}

/// Quem esta máquina é, para responder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anuncio {
    /// O identificador da instalação.
    pub maquina: String,
    /// O nome da máquina.
    pub nome: String,
    /// A porta em que o serviço atende.
    pub porta: u16,
}

/// A pergunta, em bytes.
#[must_use]
pub fn pergunta() -> Vec<u8> {
    let mut bytes = MAGICA.to_vec();
    bytes.extend([PERGUNTA, VERSAO]);
    bytes
}

/// Se estes bytes são uma pergunta.
#[must_use]
pub fn e_pergunta(bytes: &[u8]) -> bool {
    bytes.strip_prefix(MAGICA.as_slice()) == Some(&[PERGUNTA, VERSAO])
}

/// A resposta, em bytes. Nome e id longos demais são cortados, nunca recusados.
#[must_use]
pub fn resposta(anuncio: &Anuncio) -> Vec<u8> {
    let mut bytes = MAGICA.to_vec();
    bytes.extend([RESPOSTA, VERSAO]);
    bytes.extend(anuncio.porta.to_le_bytes());
    for campo in [&anuncio.maquina, &anuncio.nome] {
        let corte = cortar(campo, NOME_MAXIMO);
        bytes.push(u8::try_from(corte.len()).unwrap_or(0));
        bytes.extend(corte.as_bytes());
    }
    bytes
}

/// Lê uma resposta, vinda de `de`. `None` para qualquer coisa fora do formato.
#[must_use]
pub fn ler_resposta(bytes: &[u8], de: SocketAddr) -> Option<Candidate> {
    let resto = bytes.strip_prefix(MAGICA.as_slice())?;
    let (&[tipo, versao], resto) = resto.split_first_chunk::<2>()?;
    if tipo != RESPOSTA || versao != VERSAO {
        return None;
    }
    let (porta, resto) = resto.split_first_chunk::<2>()?;
    let porta = u16::from_le_bytes(*porta);
    let (maquina, resto) = ler_campo(resto)?;
    let (nome, _) = ler_campo(resto)?;
    if porta == 0 || maquina.is_empty() {
        return None;
    }
    Some(Candidate {
        label: if nome.is_empty() {
            maquina.clone()
        } else {
            nome
        },
        machine: maquina,
        addr: SocketAddr::new(de.ip(), porta),
    })
}

/// Um campo: um byte de tamanho e o texto.
fn ler_campo(bytes: &[u8]) -> Option<(String, &[u8])> {
    let (&tamanho, resto) = bytes.split_first()?;
    let tamanho = usize::from(tamanho);
    if tamanho > NOME_MAXIMO {
        return None;
    }
    let (texto, resto) = resto.split_at_checked(tamanho)?;
    Some((std::str::from_utf8(texto).ok()?.to_owned(), resto))
}

/// O maior prefixo de `texto` com até `maximo` bytes, sem partir um caractere.
fn cortar(texto: &str, maximo: usize) -> &str {
    let mut fim = texto.len().min(maximo);
    while !texto.is_char_boundary(fim) {
        fim -= 1;
    }
    texto.get(..fim).unwrap_or("")
}

/// Responde às perguntas que chegarem a `socket`, para sempre.
///
/// Uma resposta por pergunta, para o endereço que perguntou. Um datagrama que não é pergunta é
/// ignorado: esta porta não tem outro uso.
pub async fn responder(socket: UdpSocket, anuncio: Anuncio) {
    let resposta = resposta(&anuncio);
    let mut buf = [0u8; 64];
    loop {
        match socket.recv_from(&mut buf).await {
            Ok((tamanho, de)) if buf.get(..tamanho).is_some_and(e_pergunta) => {
                let _ = socket.send_to(&resposta, de).await;
            }
            // Outro tráfego, ou o ICMP de um envio anterior (ver `handshake`): segue.
            Ok(_) | Err(_) => {}
        }
    }
}

/// Abre o socket de resposta, na porta da descoberta, em todas as interfaces.
///
/// # Errors
///
/// [`crate::NetError::Io`] se a porta não puder ser aberta — outra instância do serviço, por
/// exemplo.
pub async fn abrir_resposta() -> Result<UdpSocket> {
    let endereco = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), PORTA_DA_DESCOBERTA);
    Ok(UdpSocket::bind(endereco).await?)
}

/// Pergunta a `destinos` e junta as respostas que chegarem em `duracao`.
///
/// A pergunta vai duas vezes, com um intervalo: um broadcast perdido no Wi-Fi é comum, e duas
/// tentativas custam nada.
///
/// # Errors
///
/// [`crate::NetError::Io`] se o socket de busca não abrir.
pub async fn procurar_em(destinos: &[SocketAddr], duracao: Duration) -> Result<Vec<Candidate>> {
    let socket = UdpSocket::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0)).await?;
    socket.set_broadcast(true)?;
    let pergunta = pergunta();
    let mut achados: Vec<Candidate> = Vec::new();
    let limite = tokio::time::Instant::now() + duracao;
    let mut buf = [0u8; 256];
    for rodada in 0..2u32 {
        for destino in destinos {
            let _ = socket.send_to(&pergunta, destino).await;
        }
        let fim_da_rodada = if rodada == 0 {
            tokio::time::Instant::now() + duracao / 2
        } else {
            limite
        };
        loop {
            let (tamanho, de) =
                match tokio::time::timeout_at(fim_da_rodada, socket.recv_from(&mut buf)).await {
                    Ok(Ok(recebido)) => recebido,
                    // O ICMP de uma pergunta sem resposta, no Windows (ver `handshake`).
                    Ok(Err(erro)) if erro.kind() == std::io::ErrorKind::ConnectionReset => continue,
                    Ok(Err(_)) | Err(_) => break,
                };
            let Some(candidato) = buf.get(..tamanho).and_then(|b| ler_resposta(b, de)) else {
                continue;
            };
            if !achados.iter().any(|c| c.machine == candidato.machine) {
                achados.push(candidato);
            }
        }
    }
    Ok(achados)
}

/// Pergunta à rede local inteira: o broadcast de cada sub-rede IPv4 desta máquina, e o geral.
///
/// # Errors
///
/// Como [`procurar_em`].
pub async fn procurar(duracao: Duration) -> Result<Vec<Candidate>> {
    procurar_em(&destinos_de_broadcast(), duracao).await
}

/// Para onde perguntar: o broadcast dirigido de cada interface IPv4 (o geral, `255.255.255.255`,
/// sai só pela interface da rota padrão em alguns sistemas) e o geral também.
fn destinos_de_broadcast() -> Vec<SocketAddr> {
    let mut destinos: Vec<SocketAddr> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|interface| match interface.addr {
            if_addrs::IfAddr::V4(v4) if !v4.ip.is_loopback() => {
                Some(broadcast_de(v4.ip, v4.netmask))
            }
            _ => None,
        })
        .map(|ip| SocketAddr::new(IpAddr::V4(ip), PORTA_DA_DESCOBERTA))
        .collect();
    destinos.push(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::BROADCAST),
        PORTA_DA_DESCOBERTA,
    ));
    destinos.dedup();
    destinos
}

/// O endereço de broadcast de uma sub-rede.
fn broadcast_de(ip: Ipv4Addr, mascara: Ipv4Addr) -> Ipv4Addr {
    Ipv4Addr::from(u32::from(ip) | !u32::from(mascara))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anuncio(nome: &str) -> Anuncio {
        Anuncio {
            maquina: "c18c07a272f72aac2c04c12b923eb7d8".to_owned(),
            nome: nome.to_owned(),
            porta: 52525,
        }
    }

    fn de() -> SocketAddr {
        "10.0.0.135:40000"
            .parse()
            .unwrap_or_else(|_| unreachable!())
    }

    #[test]
    fn a_resposta_vai_e_volta_com_o_endereco_de_quem_respondeu() {
        let lido = ler_resposta(&resposta(&anuncio("fedora")), de());
        assert_eq!(
            lido,
            Some(Candidate {
                label: "fedora".to_owned(),
                machine: "c18c07a272f72aac2c04c12b923eb7d8".to_owned(),
                addr: "10.0.0.135:52525"
                    .parse()
                    .unwrap_or_else(|_| unreachable!()),
            })
        );
    }

    #[test]
    fn nome_comprido_e_cortado_sem_partir_caractere() {
        let longo = "ç".repeat(100);
        let lido = ler_resposta(&resposta(&anuncio(&longo)), de()).map(|c| c.label);
        let lido = lido.unwrap_or_default();
        assert!(
            lido.len() <= NOME_MAXIMO && lido.chars().all(|c| c == 'ç'),
            "{lido}"
        );
    }

    #[test]
    fn so_a_pergunta_e_pergunta() {
        assert!(e_pergunta(&pergunta()));
        assert!(!e_pergunta(b"IRDESC?\x02"));
        assert!(!e_pergunta(&resposta(&anuncio("x"))));
        assert!(!e_pergunta(b"\x00\x01handshake"));
    }

    #[test]
    fn resposta_truncada_ou_forjada_nao_vira_candidato() {
        let inteira = resposta(&anuncio("fedora"));
        for tamanho in 0..inteira.len() {
            assert_eq!(
                ler_resposta(inteira.get(..tamanho).unwrap_or(&[]), de()),
                None,
                "{tamanho}"
            );
        }
        let mut campo_gigante = inteira.get(..10).unwrap_or(&[]).to_vec();
        campo_gigante.push(200);
        assert_eq!(ler_resposta(&campo_gigante, de()), None);
    }

    #[test]
    fn o_broadcast_da_sub_rede() {
        assert_eq!(
            broadcast_de(
                Ipv4Addr::new(10, 0, 0, 135),
                Ipv4Addr::new(255, 255, 255, 0)
            ),
            Ipv4Addr::new(10, 0, 0, 255)
        );
    }

    #[tokio::test]
    async fn quem_escuta_responde_e_quem_pergunta_acha() {
        // Sem broadcast no teste: a pergunta vai direto ao endereço de quem responde.
        let socket = UdpSocket::bind("127.0.0.1:0")
            .await
            .unwrap_or_else(|_| unreachable!());
        let endereco = socket.local_addr().unwrap_or_else(|_| unreachable!());
        tokio::spawn(responder(socket, anuncio("fedora")));
        let achados = procurar_em(&[endereco], Duration::from_millis(400))
            .await
            .unwrap_or_default();
        assert_eq!(achados.len(), 1, "{achados:?}");
        assert_eq!(achados.first().map(|c| c.label.as_str()), Some("fedora"));
        assert_eq!(achados.first().map(|c| c.addr.port()), Some(52525));
    }
}
