//! O clipboard do sistema: modelos canônicos e backends por SO.
//!
//! # Ctrl+C e Ctrl+V **não são interceptados**
//!
//! É a decisão mais importante deste crate, e ela é sobre o que *não* fazer.
//!
//! O produto já tem ganchos de teclado no Windows e captura pelo portal no Linux. Seria possível
//! reconhecer Ctrl+C e Ctrl+V ali e agir. **Está errado**, por três razões:
//!
//! 1. **Não é o atalho que copia, é o aplicativo.** Ctrl+C no Explorer coloca `CF_HDROP`; no editor,
//!    texto; numa tela de desenho, imagem. Quem sabe o que copiar é ele, e um gancho que tentasse
//!    adivinhar erraria em cada aplicativo que usa outro atalho — e há muitos, de Ctrl+Insert a
//!    menus de contexto.
//! 2. **O atalho não é universal.** Copiar do menu, arrastar, ou o Ctrl+C de um terminal que o
//!    trata como interrupção: nenhum passaria por um reconhecedor de combinação.
//! 3. **O sistema já avisa.** `AddClipboardFormatListener` no Windows e os protocolos de dados no
//!    Wayland existem exatamente para dizer "o clipboard mudou". Usar o aviso do sistema é mais
//!    simples *e* mais correto que inferi-lo do teclado.
//!
//! Então o desenho é espelhar: o clipboard local mudou, oferecemos ao par; o par ofereceu,
//! publicamos no clipboard local. O Ctrl+V do usuário é tratado pelo aplicativo dele, lendo o
//! clipboard que já está lá — **nenhum código nosso corre no caminho da colagem**, e é por isso que
//! colar é instantâneo.
//!
//! # O laço que isso cria, e a guarda que o desfaz
//!
//! Espelhar nos dois sentidos é um laço infinito por construção: publicamos, o nosso próprio
//! ouvinte dispara, oferecemos de volta. Ver [`Eco`].
//!
//! # A forma
//!
//! ```text
//! Vigia::proxima()   →  "o clipboard mudou"          (bloqueia até mudar)
//! Clipboard::ler()   →  Conteudo, já canônico
//! Eco::oferecer()    →  se isto vale contar ao par
//!
//! Eco::publicamos()  →  antes de escrever, sempre
//! Clipboard::publicar()
//! ```

// Sem `forbid`: o backend do Windows precisa de `unsafe`, e `forbid` não pode ser afrouxado nem
// dentro de um módulo. O workspace já **nega** `unsafe_code`, e a liberação fica confinada a
// `windows::area` e `windows::vigia`, com a justificativa em cada um ([09, §4]).
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

pub mod conteudo;
pub mod eco;
pub mod error;

#[cfg(windows)]
pub mod windows;

#[cfg(not(windows))]
pub mod linux;

pub use conteudo::{Conteudo, normalizar_quebras, quebras_nativas};
pub use eco::Eco;
pub use error::{ClipError, Result};

/// Ler e escrever o clipboard desta sessão.
pub trait Clipboard: Send {
    /// O que há no clipboard agora, já na forma canônica.
    ///
    /// `None` quando há algo lá que o protocolo não transporta — um formato privado de um
    /// aplicativo, por exemplo. Não é erro: é o caso comum de "não é para nós".
    ///
    /// # Errors
    ///
    /// [`ClipError::Ocupado`] quando outro programa tem o clipboard aberto;
    /// [`ClipError::Indisponivel`] onde não há clipboard de usuário.
    fn ler(&mut self) -> Result<Option<Conteudo>>;

    /// Põe isto no clipboard, na forma nativa da plataforma.
    ///
    /// # Errors
    ///
    /// Como [`Self::ler`].
    fn publicar(&mut self, conteudo: &Conteudo) -> Result<()>;
}

/// Esperar que o clipboard mude, sem *polling*.
///
/// *Polling* está proibido por [05, §6](../../../docs/05-windows.md), e a razão é medida: um laço
/// que acorda para comparar o clipboard gasta bateria o tempo todo para descobrir, na maioria das
/// vezes, que nada mudou. O sistema sabe avisar.
///
/// # Não é `Send`, e isso é a regra sendo cobrada
///
/// No Windows o aviso chega na fila de mensagens da **thread que criou a janela**, e `GetMessageW`
/// só lê a fila da thread que o chama. Um vigia criado numa thread e bombeado em outra não dá erro:
/// ele fica calado para sempre, que é a forma mais difícil possível de descobrir o problema.
///
/// Sem `Send`, o compilador obriga [`vigiar`] a ser chamado **na** thread que vai esperar. Um
/// `unsafe impl Send` compilaria e esconderia exatamente isto.
pub trait Vigia {
    /// Bloqueia até o clipboard mudar.
    ///
    /// # Errors
    ///
    /// [`ClipError::Indisponivel`] se o aviso do sistema não puder ser registrado.
    fn proxima(&mut self) -> Result<()>;
}

/// Abre o clipboard desta plataforma.
///
/// # Errors
///
/// [`ClipError::Indisponivel`] onde não há clipboard alcançável.
pub fn abrir() -> Result<Box<dyn Clipboard>> {
    #[cfg(windows)]
    {
        windows::abrir()
    }
    #[cfg(not(windows))]
    {
        linux::abrir()
    }
}

/// Começa a vigiar as mudanças do clipboard.
///
/// # Errors
///
/// [`ClipError::Indisponivel`] onde o aviso do sistema não existe.
pub fn vigiar() -> Result<Box<dyn Vigia>> {
    #[cfg(windows)]
    {
        windows::vigiar()
    }
    #[cfg(not(windows))]
    {
        linux::vigiar()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Um clipboard de mentira, para exercitar o ciclo sem sistema operacional nenhum.
    #[derive(Debug, Default)]
    struct Mentira {
        dentro: Option<Conteudo>,
        publicacoes: usize,
    }

    impl Clipboard for Mentira {
        fn ler(&mut self) -> Result<Option<Conteudo>> {
            Ok(self.dentro.clone())
        }

        fn publicar(&mut self, conteudo: &Conteudo) -> Result<()> {
            // Como o sistema de verdade: o que é publicado volta na leitura seguinte, e com as
            // quebras nativas.
            self.dentro = Some(match conteudo {
                Conteudo::Texto(texto) => Conteudo::Texto(quebras_nativas(texto)),
                outro => outro.clone(),
            });
            self.publicacoes += 1;
            Ok(())
        }
    }

    #[test]
    fn o_ciclo_inteiro_nao_entra_em_laco() {
        // A simulação do defeito que a guarda existe para impedir: publicar o que veio do par, o
        // ouvinte disparar, e o ciclo se repetir. Aqui ele tem de parar na primeira volta.
        let mut clip = Mentira::default();
        let mut eco = Eco::nova();
        let do_par = Conteudo::Texto("veio de lá\ncom duas linhas".to_owned());

        eco.publicamos(&do_par);
        clip.publicar(&do_par).unwrap();

        // O ouvinte dispara. Cinco vezes, como o Windows pode fazer.
        for volta in 1..=5 {
            let agora = clip.ler().unwrap().expect("há conteúdo");
            // Normaliza como o backend faz ao ler.
            let agora = match agora {
                Conteudo::Texto(bruto) => Conteudo::texto(&bruto),
                outro => outro,
            };
            assert!(
                !eco.oferecer(&agora),
                "a volta {volta} escapou e o laço teria começado"
            );
        }
        assert_eq!(clip.publicacoes, 1, "publicamos uma vez, e uma só");
    }

    #[test]
    fn uma_copia_do_usuario_depois_de_receber_e_oferecida() {
        let mut clip = Mentira::default();
        let mut eco = Eco::nova();
        eco.publicamos(&Conteudo::Texto("recebido".to_owned()));
        clip.publicar(&Conteudo::Texto("recebido".to_owned()))
            .unwrap();

        // O usuário copia outra coisa.
        clip.dentro = Some(Conteudo::Texto("o que eu copiei".to_owned()));
        let agora = clip.ler().unwrap().expect("há conteúdo");
        assert!(eco.oferecer(&agora));
    }
}
