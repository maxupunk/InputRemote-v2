//! A guarda de eco: o que nós publicamos não volta como cópia nova.
//!
//! O requisito é uma linha de [01, §3.2](../../../docs/01-visao-e-escopo.md): *"Sem laços de eco:
//! uma cópia recebida não é reanunciada como cópia nova."*
//!
//! Sem ela, o produto tem um laço infinito, e não um defeito ocasional:
//!
//! ```text
//! A: usuário aperta Ctrl+C          →  A oferece a B
//! B: publica no clipboard dele
//! B: o próprio ouvinte de B dispara →  B oferece a A
//! A: publica no clipboard dele
//! A: o próprio ouvinte de A dispara →  A oferece a B
//! ...
//! ```
//!
//! Cada volta atravessa a rede e reescreve o clipboard das duas máquinas. Não é teórico: é o que
//! acontece por padrão em qualquer implementação que apenas espelhe.
//!
//! # A regra, e por que ela é assim
//!
//! Lembra-se o resumo do que foi publicado, e **qualquer** mudança com aquele resumo é engolida —
//! não só a primeira.
//!
//! Engolir só a primeira seria errado no Windows: publicar um conteúdo com mais de um formato
//! (`CF_UNICODETEXT` e `CF_TEXT`, por exemplo) pode fazer `WM_CLIPBOARDUPDATE` disparar mais de uma
//! vez para a **mesma** cópia, e a segunda passaria. Guardar o resumo até aparecer conteúdo
//! diferente resolve as duas coisas de uma vez.
//!
//! # O que se perde, e é aceito
//!
//! Se o usuário copiar localmente **exatamente** o que acabou de receber, a cópia é engolida e o par
//! não é avisado. É inofensivo: o par já tem aquele conteúdo — foi ele que o mandou.

use tracing::debug;

use crate::conteudo::Conteudo;

/// Lembra o que esta máquina publicou, para não reanunciar.
#[derive(Debug, Default)]
pub struct Eco {
    publicado: Option<[u8; 32]>,
}

impl Eco {
    /// Uma guarda sem nada lembrado.
    #[must_use]
    pub const fn nova() -> Self {
        Self { publicado: None }
    }

    /// Registra que acabamos de publicar isto.
    ///
    /// Chamado **antes** de escrever no clipboard, e não depois: o ouvinte pode disparar antes de a
    /// chamada de publicação retornar, e aí a guarda ainda não saberia o que engolir.
    pub fn publicamos(&mut self, conteudo: &Conteudo) {
        self.publicado = Some(conteudo.resumo());
    }

    /// Se esta mudança deve ser oferecida ao par.
    ///
    /// Devolve `false` para o que nós mesmos publicamos, e para conteúdo vazio.
    pub fn oferecer(&mut self, conteudo: &Conteudo) -> bool {
        if conteudo.vazio() {
            // Esvaziar o clipboard não é conteúdo novo. Oferecer o vazio apagaria o do par.
            return false;
        }
        let resumo = conteudo.resumo();
        if self.publicado == Some(resumo) {
            // Registrado em `debug` e sem o conteúdo: clipboard registra tipo e tamanho, nunca o
            // que há dentro ([04, §7](../../../docs/04-seguranca.md)).
            debug!(
                tipo = conteudo.tipo().name(),
                tamanho = conteudo.tamanho(),
                "mudança de clipboard é a nossa própria cópia; não reanunciando"
            );
            return false;
        }
        // Conteúdo diferente: o que estava lembrado deixou de ser o que está no clipboard, e
        // guardá-lo só criaria uma chance de engolir uma cópia legítima mais tarde.
        self.publicado = None;
        true
    }

    /// Esquece o que foi publicado.
    ///
    /// Usado na queda da sessão: sem par, não há de quem vir eco, e um resumo velho só poderia
    /// engolir uma cópia legítima do usuário.
    pub fn esquecer(&mut self) {
        self.publicado = None;
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn texto(s: &str) -> Conteudo {
        Conteudo::texto(s)
    }

    #[test]
    fn uma_copia_do_usuario_e_oferecida() {
        let mut eco = Eco::nova();
        assert!(eco.oferecer(&texto("copiei isto")));
    }

    #[test]
    fn o_que_nos_publicamos_nao_volta() {
        // O caso central: sem esta linha, o Ctrl+C entra em laço entre as duas máquinas.
        let mut eco = Eco::nova();
        let recebido = texto("veio do outro computador");
        eco.publicamos(&recebido);
        assert!(!eco.oferecer(&recebido));
    }

    #[test]
    fn varios_disparos_da_mesma_publicacao_sao_todos_engolidos() {
        // No Windows, publicar com mais de um formato pode disparar `WM_CLIPBOARDUPDATE` mais de
        // uma vez para a mesma cópia. Engolir só o primeiro deixaria o laço passar pelo segundo.
        let mut eco = Eco::nova();
        let recebido = texto("uma cópia, vários avisos");
        eco.publicamos(&recebido);
        for tentativa in 1..=5 {
            assert!(
                !eco.oferecer(&recebido),
                "o disparo {tentativa} escapou da guarda"
            );
        }
    }

    #[test]
    fn conteudo_diferente_passa_mesmo_depois_de_publicarmos() {
        let mut eco = Eco::nova();
        eco.publicamos(&texto("o que recebemos"));
        assert!(eco.oferecer(&texto("o que o usuário copiou depois")));
    }

    #[test]
    fn depois_de_conteudo_diferente_o_antigo_volta_a_ser_oferecivel() {
        // A guarda protege contra o eco imediato, e não contra o usuário copiar a mesma coisa de
        // novo mais tarde — que é uma ação legítima e deve chegar ao par.
        let mut eco = Eco::nova();
        let recebido = texto("original");
        eco.publicamos(&recebido);
        assert!(!eco.oferecer(&recebido));
        assert!(eco.oferecer(&texto("outra coisa")));
        assert!(eco.oferecer(&recebido), "agora é cópia do usuário");
    }

    #[test]
    fn o_clipboard_esvaziado_nao_e_oferecido() {
        // Oferecer o vazio apagaria o clipboard do par — uma perda de dado dele, causada por uma
        // ação nossa que ele não pediu.
        let mut eco = Eco::nova();
        assert!(!eco.oferecer(&Conteudo::Texto(String::new())));
        assert!(!eco.oferecer(&Conteudo::Arquivos(Vec::new())));
        assert!(!eco.oferecer(&Conteudo::Imagem(Vec::new())));
    }

    #[test]
    fn a_guarda_distingue_tipos_com_os_mesmos_bytes() {
        let mut eco = Eco::nova();
        eco.publicamos(&Conteudo::Texto("a".to_owned()));
        assert!(
            eco.oferecer(&Conteudo::Imagem(b"a".to_vec())),
            "uma imagem não é o texto que publicamos"
        );
    }

    #[test]
    fn arquivos_publicados_tambem_nao_voltam() {
        // O caso de arquivos é o que mais importa: aqui o eco custaria uma transferência inteira,
        // e não uma mensagem de texto.
        let mut eco = Eco::nova();
        let materializados = Conteudo::Arquivos(vec![
            PathBuf::from("C:/ProgramData/InputRemote/recebidos/relatório/a.pdf"),
            PathBuf::from("C:/ProgramData/InputRemote/recebidos/relatório/b.bin"),
        ]);
        eco.publicamos(&materializados);
        assert!(!eco.oferecer(&materializados));
    }

    #[test]
    fn esquecer_faz_a_copia_voltar_a_ser_oferecivel() {
        let mut eco = Eco::nova();
        let recebido = texto("da sessão anterior");
        eco.publicamos(&recebido);
        assert!(!eco.oferecer(&recebido));
        eco.esquecer();
        assert!(
            eco.oferecer(&recebido),
            "sem par não há de quem vir eco, e um resumo velho só engoliria cópia legítima"
        );
    }

    #[test]
    fn o_texto_volta_do_sistema_com_crlf_e_ainda_e_reconhecido() {
        // O ciclo real no Windows: publicamos LF, o sistema devolve CRLF, o backend normaliza. Se
        // qualquer elo faltasse, a guarda não reconheceria a própria cópia — e o laço aconteceria.
        let mut eco = Eco::nova();
        let canonico = Conteudo::Texto("linha um\nlinha dois".to_owned());
        eco.publicamos(&canonico);
        let como_o_sistema_devolve = Conteudo::texto("linha um\r\nlinha dois");
        assert!(!eco.oferecer(&como_o_sistema_devolve));
    }
}
