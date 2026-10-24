//! O que um recado deixa a pessoa fazer sem abrir a janela: os botões da notificação e os itens do
//! menu da bandeja.
//!
//! Cada ação existe uma vez, aqui, com as três coisas que os dois lados precisam combinar: o
//! **rótulo** que a pessoa lê, o **endereço** `inputremote:` que o botão da notificação do Windows
//! abre, e o **pedido** ao serviço que ela vira. Quem monta o botão e quem atende o clique leem do
//! mesmo lugar — um endereço montado de um jeito e lido de outro seria um botão que não faz nada,
//! sem erro nenhum.
//!
//! # Por que um endereço, e não um aviso dentro do processo
//!
//! O botão da notificação do Windows abre um endereço pelo próprio Windows (ativação por
//! protocolo), como o "Abrir a pasta" já faz com `file:`. Funciona com a interface em qualquer
//! estado — e mesmo depois de ela sair, se a pessoa clica no recado que ficou na central. O
//! endereço só **pede**: quem decide se há o que parar é o serviço, com a mesma autoridade de quem
//! clicou. Um endereço `inputremote:` aberto por outra página faria, no máximo, parar uma cópia.

use ir_ipc::Pedido;
use ir_ipc::transferencia::Transferencia;

/// O esquema dos endereços das ações, registrado para o usuário pela interface.
pub const ESQUEMA: &str = "inputremote";

/// O argumento com que o Windows abre a interface para atender o botão de um recado:
/// `inputremote-ui --acao inputremote:cancelar-copia`. Um lugar só, para quem registra e quem lê.
pub const ARGUMENTO: &str = "--acao";

/// Uma ação que o recado oferece.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Acao {
    /// Parar a cópia de arquivos em curso — saindo daqui, ou chegando.
    CancelarCopia,
}

impl Acao {
    /// Todas as ações, para quem precisa decodificar um endereço.
    const TODAS: [Self; 1] = [Self::CancelarCopia];

    /// A ação que uma cópia oferece agora: parar, enquanto ela acontece, dos dois lados
    /// ([`Transferencia::cancelavel`]). A mesma para a notificação e para as duas bandejas.
    #[must_use]
    pub const fn da_copia(copia: &Transferencia) -> Option<Self> {
        if copia.cancelavel() {
            Some(Self::CancelarCopia)
        } else {
            None
        }
    }

    /// O nome da ação no endereço.
    const fn nome(self) -> &'static str {
        match self {
            Self::CancelarCopia => "cancelar-copia",
        }
    }

    /// O que a pessoa lê no menu da bandeja.
    #[must_use]
    pub const fn rotulo(self) -> &'static str {
        match self {
            Self::CancelarCopia => "Cancelar a cópia",
        }
    }

    /// O que a pessoa lê no botão da notificação, onde cabe pouco.
    #[must_use]
    pub const fn rotulo_curto(self) -> &'static str {
        match self {
            Self::CancelarCopia => "Cancelar",
        }
    }

    /// O endereço que o botão da notificação abre.
    #[must_use]
    pub fn endereco(self) -> String {
        format!("{ESQUEMA}:{}", self.nome())
    }

    /// A ação de um endereço; `None` para o que não é uma.
    ///
    /// Aceita a barra que alguns sistemas acrescentam (`inputremote:cancelar-copia/`), e só isso:
    /// o endereço vem de fora do processo, e o que não é exatamente uma ação conhecida é ignorado.
    #[must_use]
    pub fn do_endereco(endereco: &str) -> Option<Self> {
        let resto = endereco.trim().strip_prefix(ESQUEMA)?.strip_prefix(':')?;
        let nome = resto.trim_start_matches('/').trim_end_matches('/');
        Self::TODAS.into_iter().find(|acao| acao.nome() == nome)
    }

    /// A ação que a linha de comando deste processo pede (`--acao <endereço>`): é assim que o
    /// Windows abre a interface para atender o botão de um recado.
    #[must_use]
    pub fn da_linha_de_comando() -> Option<Self> {
        let mut argumentos = std::env::args().skip(1);
        argumentos.find(|argumento| argumento == ARGUMENTO)?;
        Self::do_endereco(&argumentos.next()?)
    }

    /// O pedido ao serviço que esta ação é.
    #[must_use]
    pub const fn pedido(self) -> Pedido {
        match self {
            Self::CancelarCopia => Pedido::CancelarCopia,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_endereco_montado_e_o_endereco_lido() {
        for acao in Acao::TODAS {
            assert_eq!(Acao::do_endereco(&acao.endereco()), Some(acao));
        }
        assert_eq!(
            Acao::do_endereco("inputremote:cancelar-copia/"),
            Some(Acao::CancelarCopia)
        );
        assert_eq!(
            Acao::do_endereco("inputremote://cancelar-copia"),
            Some(Acao::CancelarCopia)
        );
    }

    #[test]
    fn endereco_estranho_nao_vira_acao() {
        for estranho in [
            "",
            "inputremote:",
            "inputremote:apagar-tudo",
            "inputremote:cancelar-copia&x=1",
            "file:///cancelar-copia",
            "cancelar-copia",
        ] {
            assert_eq!(Acao::do_endereco(estranho), None, "{estranho}");
        }
    }

    #[test]
    fn cancelar_e_o_pedido_de_cancelar() {
        assert_eq!(Acao::CancelarCopia.pedido(), Pedido::CancelarCopia);
    }
}
