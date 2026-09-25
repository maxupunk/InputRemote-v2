//! Transferência de arquivos: manifesto, blocos, BLAKE3, cotas e staging.
//!
//! Este crate é o motor do canal 5 do protocolo. Ele **não** cifra e **não** conhece socket: ele
//! produz e consome [`BulkMessage`](ir_proto::message::BulkMessage), e quem os leva pelo TCP é o
//! `ir-transporte` ([02, §2](../../../docs/02-arquitetura.md)).
//!
//! # A forma: puxar, não empurrar
//!
//! Os dois lados são máquinas de estado que respondem a uma pergunta por vez.
//!
//! ```text
//! enviar                                        receber
//! ------                                        -------
//! manifesto::montar(id, o que o usuário copiou)
//! Envio::novo(plano)
//! envio.manifesto()                    ──────►  Recepcao::abrir(...)   cota, permissão, disco
//!                                      ◄──────  Accept | Reject
//! envio.proxima()  FileStart           ──────►  recepcao.aplicar(...)
//! envio.proxima()  FileBlock  ×N       ──────►  escreve e resume
//! envio.proxima()  FileEnd    BLAKE3   ──────►  confere o resumo
//!                                      ◄──────  Verified
//! envio.proxima()  None                         recepcao.concluir()    um `rename` só
//! ```
//!
//! Quem chama decide quando pedir a próxima mensagem, e na prática isso é quando o socket aceitou a
//! anterior. É de onde vem a contrapressão: numa transferência de 5 GB, o disco nunca vai à frente
//! da rede, porque é a rede que pede.
//!
//! # O que este crate garante
//!
//! - **Nada é escrito fora da pasta de destino.** O caminho relativo é conferido na cota e **de
//!   novo** na hora de escrever, que é onde a consequência está.
//! - **Nada além do que foi aceito.** Cada bloco é conferido contra o tamanho declarado do item, o
//!   que impede um par de anunciar um byte e mandar gigabytes.
//! - **Nem árvore parcial nem arquivo temporário.** A montagem se apaga no `Drop`, então é o que
//!   acontece quando não se faz nada — e caminho de erro é exatamente onde a chamada de limpeza é
//!   esquecida.
//! - **Publicação sem meio-caminho visível.** Um `rename` no fim: antes dele não havia nada no
//!   destino, depois dele está tudo.
//!
//! # O que ele deliberadamente não faz
//!
//! Não retoma transferência interrompida. Se o resumo não conferir ou o enlace cair, a árvore vai
//! embora e a cópia é refeita. Retomada exigiria confiar num estado parcial em disco entre duas
//! execuções, e a garantia que o usuário pediu foi **cópia certa**, não cópia rápida na segunda
//! tentativa.

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

pub mod cota;
mod em_curso;
pub mod envio;
pub mod error;
pub mod manifesto;
pub mod permissao;
pub mod publicacao;
pub mod recepcao;
pub mod staging;

#[cfg(test)]
mod teste;

pub use cota::{Cota, EspacoLivre};
pub use envio::Envio;
pub use error::{FileError, Result};
pub use manifesto::Plano;
pub use permissao::{Autorizacao, Leitor};
pub use publicacao::Publicacao;
pub use recepcao::{Abertura, Reacao, Recepcao};
pub use staging::Staging;
