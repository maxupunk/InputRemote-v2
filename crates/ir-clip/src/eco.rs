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

/// Lembra o que esta máquina publicou e o que ela já ofereceu, para não repetir nenhum dos dois.
///
/// # Dois resumos, e por que o segundo existe
///
/// O primeiro, `publicado`, é a guarda de eco: o que veio do par não volta para ele.
///
/// O segundo, `oferecido`, apareceu com a sincronização na travessia
/// ([ADR-0011](../../../docs/adr/0011-clipboard-na-travessia.md)). O clipboard é lido **toda vez**
/// que o controle sai desta máquina, e o usuário atravessa a borda dezenas de vezes por minuto.
/// Sem este resumo, cada travessia reenviaria o mesmo conteúdo — com uma pasta de 2 GB no
/// clipboard, cada ida do mouse até a outra tela seria uma transferência de 2 GB.
#[derive(Debug, Default)]
pub struct Eco {
    publicado: Option<[u8; 32]>,
    oferecido: Option<[u8; 32]>,
}

impl Eco {
    /// Uma guarda sem nada lembrado.
    #[must_use]
    pub const fn nova() -> Self {
        Self {
            publicado: None,
            oferecido: None,
        }
    }

    /// Registra que acabamos de publicar isto.
    ///
    /// Chamado **antes** de escrever no clipboard, e não depois: o ouvinte pode disparar antes de a
    /// chamada de publicação retornar, e aí a guarda ainda não saberia o que engolir.
    ///
    /// Também esquece o que tínhamos oferecido. O par agora tem **outra** coisa no clipboard dele, e
    /// se o usuário copiar de novo aquilo que tínhamos mandado antes, precisa ir de novo.
    pub fn publicamos(&mut self, conteudo: &Conteudo) {
        self.publicado = Some(conteudo.resumo());
        self.oferecido = None;
    }

    /// Se esta mudança deve ser oferecida ao par. Quando devolve `true`, registra a oferta.
    ///
    /// Devolve `false` para o que nós mesmos publicamos, para o que já oferecemos, e para conteúdo
    /// vazio.
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
        if self.oferecido == Some(resumo) {
            // O par já tem isto. A travessia seguinte não é motivo para mandar de novo.
            return false;
        }
        // Conteúdo diferente: o que estava lembrado deixou de ser o que está no clipboard, e
        // guardá-lo só criaria uma chance de engolir uma cópia legítima mais tarde.
        self.publicado = None;
        self.oferecido = Some(resumo);
        true
    }

    /// A última oferta não chegou ao par.
    ///
    /// Sem isto, uma transferência que falhou ficaria marcada como entregue, e copiar a mesma coisa
    /// de novo não a mandaria — o usuário teria de copiar outra coisa e voltar.
    pub fn oferta_falhou(&mut self) {
        self.oferecido = None;
    }

    /// Esquece tudo.
    ///
    /// Usado na queda da sessão: sem par, não há de quem vir eco, e um resumo velho só poderia
    /// engolir uma cópia legítima do usuário.
    pub fn esquecer(&mut self) {
        self.publicado = None;
        self.oferecido = None;
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
    fn atravessar_de_novo_nao_reenvia_o_que_o_par_ja_tem() {
        // O defeito que o segundo resumo existe para impedir: a leitura acontece a cada travessia,
        // e o usuário atravessa dezenas de vezes por minuto.
        let mut eco = Eco::nova();
        let pasta = Conteudo::Arquivos(vec![PathBuf::from("/home/maxuel/relatório")]);
        assert!(eco.oferecer(&pasta), "a primeira vez vai");
        for travessia in 2..=20 {
            assert!(
                !eco.oferecer(&pasta),
                "a travessia {travessia} reenviaria a pasta inteira"
            );
        }
    }

    #[test]
    fn receber_outra_coisa_faz_o_antigo_voltar_a_ser_enviavel() {
        // Mandamos X, recebemos Y. Agora o par tem Y no clipboard; se o usuário copiar X de novo,
        // X precisa ir de novo — senão o par colaria Y achando que é X.
        let mut eco = Eco::nova();
        let x = texto("o que mandamos");
        assert!(eco.oferecer(&x));
        eco.publicamos(&texto("o que recebemos"));
        assert!(eco.oferecer(&x));
    }

    #[test]
    fn uma_oferta_que_falhou_pode_ser_repetida() {
        let mut eco = Eco::nova();
        let x = texto("não chegou");
        assert!(eco.oferecer(&x));
        assert!(!eco.oferecer(&x));
        eco.oferta_falhou();
        assert!(
            eco.oferecer(&x),
            "sem isto, a cópia que falhou ficaria marcada como entregue"
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
