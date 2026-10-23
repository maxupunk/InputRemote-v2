//! O que o resto do crate vê do portal: o [`Clipboard`] e o [`Vigia`], os dois sobre o mesmo
//! [`Portal`].

use std::collections::HashMap;
use std::sync::PoisonError;

use tracing::debug;
use zbus::zvariant::{OwnedObjectPath, Value};

use super::sessao::{CAMINHO, CLIPBOARD, DESTINO};
use super::{Portal, Publicado, falha, tipos};
use crate::conteudo::Conteudo;
use crate::error::{ClipError, Result};
use crate::{Clipboard, Vigia};

/// O clipboard pelo portal.
#[derive(Debug)]
struct ClipboardDoPortal {
    portal: &'static Portal,
}

impl Clipboard for ClipboardDoPortal {
    fn ler(&mut self) -> Result<Option<Conteudo>> {
        let (sessao, oferecidos) = {
            let estado = self.portal.estado();
            let sessao = Portal::sessao(&estado)?;
            // A seleção é nossa: o que está lá é o que publicamos, e pedi-la ao portal seria
            // pedir a nós mesmos, na thread que teria de responder.
            if let Some(nosso) = &estado.nosso {
                return Ok(Some(nosso.conteudo.clone()));
            }
            (sessao, estado.oferecidos.clone())
        };
        let candidatos: Vec<&str> = if oferecidos.is_empty() {
            tipos::TENTATIVAS.to_vec()
        } else {
            tipos::escolher(&oferecidos).into_iter().collect()
        };
        for tipo in candidatos {
            match self.portal.ler_tipo(&sessao, tipo) {
                Ok(bytes) => {
                    if let Some(conteudo) = tipos::conteudo(tipo, &bytes) {
                        return Ok(Some(conteudo));
                    }
                }
                Err(erro) => debug!(%erro, tipo, "o portal não leu este tipo"),
            }
        }
        Ok(None)
    }

    fn publicar(&mut self, conteudo: &Conteudo) -> Result<()> {
        let ofertas = tipos::ofertas(conteudo);
        let nomes: Vec<&str> = ofertas.iter().map(|(tipo, _)| *tipo).collect();
        let sessao = {
            let mut estado = self.portal.estado();
            if estado.recusado {
                return Portal::sessao(&estado).map(|_| ());
            }
            // Guardado antes de anunciar: quem colar logo depois do anúncio já encontra os bytes.
            estado.nosso = Some(Publicado {
                conteudo: conteudo.clone(),
                ofertas,
            });
            estado.sessao.clone()
        };
        let Some(sessao) = sessao else {
            // A sessão ainda não abriu — a tela bloqueada, o diálogo esperando. O que chegou fica
            // guardado e é anunciado quando ela abrir; antes, se perdia.
            debug!("o que chegou espera a sessão do portal abrir");
            return Ok(());
        };
        self.portal.anunciar(&sessao, &nomes)
    }
}

impl Portal {
    /// Diz ao GNOME que a seleção é nossa, com estes tipos. Os bytes vão quando alguém colar.
    pub(super) fn anunciar(&self, sessao: &OwnedObjectPath, tipos: &[&str]) -> Result<()> {
        let opcoes = HashMap::from([("mime_types", Value::from(tipos.to_vec()))]);
        self.conexao
            .call_method(
                Some(DESTINO),
                CAMINHO,
                Some(CLIPBOARD),
                "SetSelection",
                &(sessao, opcoes),
            )
            .map_err(falha)?;
        Ok(())
    }
}

/// O aviso de mudança, pelo `SelectionOwnerChanged`: o que o `wl-paste --watch` não consegue no
/// GNOME. Com ele o clipboard sai no momento da cópia, e não só na travessia.
#[derive(Debug)]
struct VigiaDoPortal {
    portal: &'static Portal,
    visto: u64,
}

impl Vigia for VigiaDoPortal {
    fn proxima(&mut self) -> Result<()> {
        let mut estado = self.portal.estado();
        loop {
            if estado.recusado {
                return Err(ClipError::Indisponivel(
                    "o clipboard não foi autorizado no diálogo do GNOME",
                ));
            }
            if estado.mudancas > self.visto {
                self.visto = estado.mudancas;
                return Ok(());
            }
            estado = self
                .portal
                .mudou
                .wait(estado)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

/// O clipboard pelo portal. A sessão abre em segundo plano; até lá, ler e publicar dizem que o
/// clipboard ainda não está disponível.
pub(in crate::linux) fn abrir(portal: &'static Portal) -> Box<dyn Clipboard> {
    portal.comecar();
    Box::new(ClipboardDoPortal { portal })
}

/// O aviso de mudança pelo portal.
pub(in crate::linux) fn vigiar(portal: &'static Portal) -> Box<dyn Vigia> {
    portal.comecar();
    let visto = portal.estado().mudancas;
    Box::new(VigiaDoPortal { portal, visto })
}
