//! Injeção em qualquer desktop: o usuário, a tela de bloqueio e o UAC.
//!
//! É a metade de injeção do [ADR-0008](../../../docs/adr/0008-agente-com-thread-por-desktop.md).
//! O Windows tem um desktop por "tela": `Default` é a área de trabalho, `Winlogon` a tela de
//! bloqueio, de login e do Ctrl+Alt+Del, e o UAC seguro também roda nela. Um `SendInput` só chega
//! ao desktop **da thread que o chama**, e uma thread só troca de desktop antes de ter janela ou
//! gancho. Por isso uma thread por desktop, presa a ele desde a primeira linha, criadas na subida:
//! quando a tela bloqueia, a única coisa que muda é qual thread recebe o próximo evento — nada é
//! criado no momento em que o usuário quer digitar a senha.
//!
//! Só injeção. Nenhuma destas threads instala gancho: capturar no `Winlogon` seria ler a senha
//! digitada na tela de bloqueio da própria máquina ([04](../../../docs/04-seguranca.md)).
//!
//! Abrir o `Winlogon` exige ser `SYSTEM`, que é como o serviço lança o agente. Em primeiro plano,
//! como usuário, só o `Default` abre — e o injetor funciona como antes, só na área de trabalho.

#![allow(unsafe_code)]
#![allow(unreachable_pub)]

use std::sync::mpsc::{Receiver, Sender, SyncSender, channel, sync_channel};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::StationsAndDesktops::{
    CloseDesktop, DESKTOP_CONTROL_FLAGS, DESKTOP_READOBJECTS, DESKTOP_WRITEOBJECTS,
    GetUserObjectInformationW, HDESK, OpenDesktopW, OpenInputDesktop, SetThreadDesktop, UOI_NAME,
};
use windows::core::HSTRING;

use super::sendinput::SendInputInjector;
use crate::error::{InputError, Result};
use crate::{InjectEvent, Injector};

/// Os desktops em que se injeta, na ordem em que as threads nascem.
const DESKTOPS: &[&str] = &["Default", "Winlogon", "Screen-saver"];

/// De quanto em quanto tempo se pergunta ao sistema qual é o desktop de entrada.
///
/// O ADR pede 200 ms; aqui a pergunta é feita na hora da injeção, com este cache, então o primeiro
/// evento depois do bloqueio já vai para a thread certa.
const VALIDADE: Duration = Duration::from_millis(100);

/// O que uma thread de desktop faz.
enum Pedido {
    Injetar(InjectEvent, SyncSender<Result<()>>),
    SoltarTudo,
}

/// Uma thread de desktop viva.
struct Thread {
    nome: &'static str,
    pedidos: Sender<Pedido>,
}

/// O injetor que segue o desktop de entrada.
pub struct InjetorPorDesktop {
    threads: Vec<Thread>,
    /// O desktop de entrada da última consulta, e quando ela foi feita.
    atual: Option<(String, Instant)>,
    /// O desktop que recebeu o último evento, para soltar tudo nele ao trocar.
    ultimo: Option<&'static str>,
    /// Se o par pode digitar fora da área de trabalho. Desligado até o serviço permitir.
    protegido_permitido: bool,
}

impl InjetorPorDesktop {
    /// Cria uma thread para cada desktop que este processo consegue abrir.
    ///
    /// # Errors
    ///
    /// [`InputError::Unsupported`] se nem o `Default` abrir.
    pub fn novo() -> Result<Self> {
        let threads: Vec<Thread> = DESKTOPS.iter().filter_map(|nome| lancar(nome)).collect();
        if !threads.iter().any(|thread| thread.nome == "Default") {
            return Err(InputError::Unsupported);
        }
        let nomes: Vec<&str> = threads.iter().map(|thread| thread.nome).collect();
        tracing::info!(?nomes, "injeção por desktop pronta");
        Ok(Self {
            threads,
            atual: None,
            ultimo: None,
            protegido_permitido: false,
        })
    }

    /// O desktop de entrada, perguntado ao sistema no máximo a cada [`VALIDADE`].
    fn desktop_de_entrada(&mut self) -> String {
        let agora = Instant::now();
        if let Some((nome, quando)) = &self.atual
            && agora.duration_since(*quando) < VALIDADE
        {
            return nome.clone();
        }
        // Sem resposta — o que acontece por instantes durante a troca —, vale a área de trabalho.
        let nome = nome_do_desktop_de_entrada().unwrap_or_else(|| "Default".to_owned());
        self.atual = Some((nome.clone(), agora));
        nome
    }

    /// A thread do desktop dado, ou a do `Default` se esse desktop não abriu aqui.
    fn thread_para(&self, desktop: &str) -> Option<&Thread> {
        escolher(&self.threads, desktop, |thread| thread.nome)
    }
}

/// Escolhe o alvo pelo nome, com o `Default` como reserva. Separado das threads para ser testado.
fn escolher<'a, T>(alvos: &'a [T], desktop: &str, nome: impl Fn(&T) -> &str) -> Option<&'a T> {
    alvos
        .iter()
        .find(|alvo| nome(alvo).eq_ignore_ascii_case(desktop))
        .or_else(|| alvos.iter().find(|alvo| nome(alvo) == "Default"))
}

impl Injector for InjetorPorDesktop {
    fn inject(&mut self, event: InjectEvent) -> Result<()> {
        let desktop = self.desktop_de_entrada();
        let Some(thread) = self.thread_para(&desktop) else {
            return Err(InputError::Unsupported);
        };
        let nome = thread.nome;
        let pedidos = thread.pedidos.clone();
        if self.ultimo.is_some_and(|ultimo| ultimo != nome) {
            // O que ficou apertado no desktop anterior não pode ficar lá quando ele voltar.
            if let Some(anterior) = self.ultimo.and_then(|ultimo| self.thread_para(ultimo)) {
                let _ = anterior.pedidos.send(Pedido::SoltarTudo);
            }
            tracing::info!(desktop = nome, "injetando em outro desktop");
        }
        self.ultimo = Some(nome);
        if nome != "Default" && !self.protegido_permitido {
            // A tela de bloqueio ou o UAC, sem a permissão do administrador desta máquina.
            return Err(InputError::Rejected);
        }
        let (resposta, recebe) = sync_channel(1);
        pedidos
            .send(Pedido::Injetar(event, resposta))
            .map_err(|_| InputError::Device(format!("a thread do desktop {nome} morreu")))?;
        recebe
            .recv()
            .map_err(|_| InputError::Device(format!("a thread do desktop {nome} morreu")))?
    }

    fn release_all(&mut self) -> Result<()> {
        for thread in &self.threads {
            let _ = thread.pedidos.send(Pedido::SoltarTudo);
        }
        Ok(())
    }

    fn desktop(&self) -> Option<String> {
        self.ultimo.map(str::to_owned)
    }

    fn desktops(&self) -> Vec<String> {
        self.threads
            .iter()
            .map(|thread| thread.nome.to_owned())
            .collect()
    }

    fn permitir_desktop_protegido(&mut self, permitir: bool) {
        self.protegido_permitido = permitir;
    }
}

/// Lança a thread de um desktop, se ele abrir. `None` quando não há permissão para ele.
fn lancar(nome: &'static str) -> Option<Thread> {
    let (pedidos, recebidos) = channel();
    let (pronta, esperar) = sync_channel(1);
    std::thread::Builder::new()
        .name(format!("desk-{}", nome.to_ascii_lowercase()))
        .spawn(move || servir(nome, &recebidos, &pronta))
        .ok()?;
    match esperar.recv() {
        Ok(true) => Some(Thread { nome, pedidos }),
        _ => None,
    }
}

/// O corpo de uma thread de desktop: prende-se a ele, e injeta o que chegar.
fn servir(nome: &'static str, recebidos: &Receiver<Pedido>, pronta: &SyncSender<bool>) {
    // Primeira ação da thread, antes de qualquer janela: depois dela, `SetThreadDesktop` falha.
    let Some(desktop) = prender_a(nome) else {
        let _ = pronta.send(false);
        return;
    };
    let _ = pronta.send(true);
    let mut injetor = SendInputInjector::new();
    while let Ok(pedido) = recebidos.recv() {
        match pedido {
            Pedido::Injetar(evento, resposta) => {
                let _ = resposta.send(injetor.inject(evento));
            }
            Pedido::SoltarTudo => {
                let _ = injetor.release_all();
            }
        }
    }
    // SAFETY: o handle veio de `OpenDesktopW` e não é mais usado; a thread está terminando.
    unsafe { CloseDesktop(desktop) }.ok();
}

/// Abre o desktop e prende a thread corrente a ele.
fn prender_a(nome: &str) -> Option<HDESK> {
    let texto = HSTRING::from(nome);
    let acesso = DESKTOP_READOBJECTS.0
        | DESKTOP_WRITEOBJECTS.0
        | windows::Win32::System::StationsAndDesktops::DESKTOP_SWITCHDESKTOP.0;
    // SAFETY: `texto` termina em nulo e vive até o fim da chamada.
    let desktop = unsafe { OpenDesktopW(&texto, DESKTOP_CONTROL_FLAGS(0), false, acesso) }.ok()?;
    // SAFETY: `desktop` é um handle válido que acabou de abrir, e esta thread ainda não tem janela.
    if unsafe { SetThreadDesktop(desktop) }.is_err() {
        // SAFETY: o handle não chegou a ser usado.
        unsafe { CloseDesktop(desktop) }.ok();
        return None;
    }
    Some(desktop)
}

/// O nome do desktop que recebe a entrada agora: `Default`, `Winlogon`, `Screen-saver`.
#[must_use]
pub fn nome_do_desktop_de_entrada() -> Option<String> {
    // SAFETY: sem pré-condição; o handle devolvido é fechado abaixo.
    let desktop = unsafe {
        OpenInputDesktop(
            DESKTOP_CONTROL_FLAGS(0),
            false,
            windows::Win32::System::StationsAndDesktops::DESKTOP_ACCESS_FLAGS(
                DESKTOP_READOBJECTS.0,
            ),
        )
    }
    .ok()?;
    let mut nome = [0u16; 64];
    let mut tamanho = 0u32;
    let bytes = u32::try_from(size_of_val(&nome)).unwrap_or(0);
    // SAFETY: `nome` é um destino de `bytes` bytes, e `tamanho` um destino válido.
    let lido = unsafe {
        GetUserObjectInformationW(
            HANDLE(desktop.0),
            UOI_NAME,
            Some(nome.as_mut_ptr().cast()),
            bytes,
            Some(std::ptr::from_mut(&mut tamanho)),
        )
    };
    // SAFETY: o handle veio de `OpenInputDesktop` e não é mais usado.
    unsafe { CloseDesktop(desktop) }.ok();
    lido.ok()?;
    let fim = nome.iter().position(|&c| c == 0).unwrap_or(nome.len());
    String::from_utf16(nome.get(..fim)?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_desktop_certo_e_escolhido_e_o_default_e_a_reserva() {
        let alvos = ["Default", "Winlogon"];
        assert_eq!(escolher(&alvos, "Winlogon", |a| a), Some(&"Winlogon"));
        assert_eq!(escolher(&alvos, "winlogon", |a| a), Some(&"Winlogon"));
        assert_eq!(
            escolher(&alvos, "Screen-saver", |a| a),
            Some(&"Default"),
            "sem thread para ele, a área de trabalho"
        );
        assert_eq!(escolher(&["Winlogon"], "Outro", |a| a), None);
    }

    #[test]
    fn o_desktop_de_entrada_e_um_dos_conhecidos() {
        // Com a tela desbloqueada é o `Default`; com o protetor de tela, `Screen-saver` — a bancada
        // já rodou este teste com ele ativo. Numa máquina sem sessão interativa, pode não haver
        // resposta.
        if let Some(nome) = nome_do_desktop_de_entrada() {
            assert!(DESKTOPS.contains(&nome.as_str()), "{nome}");
        }
    }
}
