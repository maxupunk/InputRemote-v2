//! O contrato entre o serviço, o agente e a interface.
//!
//! Este crate é a fronteira que impede o InputRemote 1 de acontecer de novo. Lá, a lógica de
//! produto morava dentro do crate da interface, que acabou com 10.491 linhas — mais que
//! transporte e plataforma somados ([00, §1](../../../docs/00-licoes-do-v1.md)).
//!
//! Aqui, `ir-ui` depende **só** disto. Ela não conhece `ir-session`, não conhece a rede, não
//! conhece Bluetooth. O que ela sabe é pedir e mostrar.
//!
//! # Dois canais, de propósito
//!
//! | Canal | Vocabulário | Quem pode falar |
//! |---|---|---|
//! | interface ↔ serviço | [`ui`] | o usuário interativo, com elevação para o que decide quem digita |
//! | agente ↔ serviço | [`agent`] | só o agente, autenticado por token e por segredo de uma via |
//!
//! `Injetar` não existe no vocabulário da interface, e essa ausência é a garantia. Se qualquer
//! processo do usuário pudesse pedir injeção, qualquer programa que ele rodasse poderia digitar
//! no prompt de UAC ([04, §5](../../../docs/04-seguranca.md)).
//!
//! # O estado é publicado, não vazado
//!
//! [`status::Estado`] **não** é o estado interno da sessão exposto. É um tipo próprio, com
//! frases prontas para a tela e sem nenhuma referência ao produto. O serviço traduz de um para
//! o outro, e é essa tradução que mantém a interface do lado de fora.
//!
//! Os tipos que a interface enxerga têm vocabulário próprio ([`vocabulario`]) e **não** são os
//! tipos do protocolo. Sem isso, `ir-ui` precisaria depender de `ir-proto`, e a interface passaria
//! a ter opinião sobre formato de fio — o que [02, §2](../../../docs/02-arquitetura.md) proíbe. A
//! exceção é [`agent`], o canal que carrega injeção de entrada, onde os tipos do protocolo são
//! exatamente os certos.
//!
//! A escolha de nomear em português os tipos deste crate é deliberada: eles descrevem o que o
//! usuário vê, e a interface que os consome está em português. Os crates de dentro
//! (`ir-proto`, `ir-session`) usam o vocabulário técnico em inglês, que é o da especificação.
//! A troca de idioma marca exatamente onde o produto termina e a apresentação começa.

#![forbid(unsafe_code)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

pub mod agent;
pub mod codec;
pub mod falha;
pub mod status;
pub mod ui;
pub mod vocabulario;

pub use agent::{ComandoDoAgente, FatoDoAgente};
pub use codec::{ErroDeCodec, MAX_MENSAGEM, PREFIXO};
pub use falha::Falha;
pub use status::{
    Estado, Latencia, LinkState, MotivoDaQueda, MotivoDoPortador, Papel, ParConhecido,
};
pub use ui::{Autoridade, Aviso, Candidato, ParaInterface, Pedido, Resposta};
pub use vocabulario::{Borda, Maquina, Nivel, Nome, Portador, Recursos};
