//! A notificação nativa do Windows: a mesma central de notificações do resto do sistema.
//!
//! Antes o recado era uma janela própria no canto da tela. Funcionava, mas não era do Windows: ignorava o "não perturbe", não ficava na central para ser lido depois, e
//! tinha a cara do InputRemote no meio da cara do sistema. A notificação nativa resolve os três, e
//! dá de graça o que a janela própria não tinha:
//!
//! - **uma barra de andamento que anda no lugar** — o recado de uma cópia é atualizado pelos dados
//!   (`NotificationData`), sem reaparecer a cada passo;
//! - **"Abrir a pasta"** no que chegou, que o próprio Windows abre (ativação por protocolo
//!   `file:`), sem a interface precisar estar viva para receber o clique.
//!
//! # Sem pacote, com o nome e o ícone certos
//!
//! Um programa sem pacote MSIX notifica por um identificador de aplicativo (AUMID) registrado em
//! `HKCU\Software\Classes\AppUserModelId\<id>`, com o nome e o ícone que a central mostra. O
//! registro é por usuário e é refeito a cada abertura — barato, e conserta um registro apagado. O
//! ícone vai para `%LOCALAPPDATA%\InputRemote`, porque a central lê um arquivo, e não um recurso.
//!
//! O texto vai sempre pelos dados, e nunca dentro do XML: o nome de um arquivo copiado pode ter
//! `<` ou `&`, e nos dados ele não é interpretado.

use std::path::{Path, PathBuf};

use crate::{Recado, Tom};
use windows::Data::Xml::Dom::XmlDocument;
use windows::UI::Notifications::{
    NotificationData, NotificationSetting, NotificationUpdateResult, ToastNotification,
    ToastNotificationManager, ToastNotifier,
};
use windows::core::HSTRING;

/// O identificador do InputRemote na central de notificações.
const AUMID: &str = "InputRemote.InputRemote";

/// O grupo de todos os recados: a etiqueta distingue um do outro dentro dele.
const GRUPO: &str = "inputremote";

/// O ícone que a central mostra ao lado do nome.
const ICONE: &[u8] = include_bytes!("../../../recursos/icone-128.png");

/// O recado com andamento: o título em cima, a barra, e o detalhe embaixo dela.
const COM_ANDAMENTO: &str = r#"<toast><visual><binding template="ToastGeneric"><text>{titulo}</text><progress value="{andamento}" valueStringOverride=" " status="{corpo}"/></binding></visual><audio silent="true"/></toast>"#;

/// A central de notificações, pronta para mostrar recados do InputRemote.
pub struct Notificacoes {
    notificador: ToastNotifier,
    /// A ordem das atualizações: a central descarta a que chega atrasada.
    sequencia: u32,
}

impl std::fmt::Debug for Notificacoes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Notificacoes")
            .field("sequencia", &self.sequencia)
            .finish_non_exhaustive()
    }
}

impl Notificacoes {
    /// Registra o InputRemote na central e abre o notificador.
    ///
    /// `None` quando o Windows não deixa: sem a API, ou com o registro recusado — e o retorno fica
    /// no ícone da bandeja e na janela. Desligadas pela pessoa nas Configurações, o notificador abre
    /// do mesmo jeito, e quem chama pergunta [`Self::desligadas_pela_pessoa`].
    #[must_use]
    pub fn abrir() -> Option<Self> {
        if let Err(erro) = registrar() {
            eprintln!("não consegui registrar as notificações do InputRemote: {erro}");
            return None;
        }
        let notificador =
            ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID)).ok()?;
        Some(Self {
            notificador,
            sequencia: 0,
        })
    }

    /// Se a pessoa desligou as notificações do InputRemote, ou todas, nas Configurações.
    #[must_use]
    pub fn desligadas_pela_pessoa(&self) -> bool {
        self.notificador.Setting().is_ok_and(|setting| {
            setting == NotificationSetting::DisabledForApplication
                || setting == NotificationSetting::DisabledForUser
        })
    }

    /// Mostra o recado. Um recado com a mesma `etiqueta` toma o lugar do anterior; sem etiqueta, ele
    /// fica ao lado dos outros.
    ///
    /// Devolve se a central aceitou.
    pub fn mostrar(&mut self, recado: &Recado, etiqueta: Option<&str>) -> bool {
        let etiqueta = etiqueta.map_or_else(|| format!("avulso-{}", self.sequencia), str::to_owned);
        let resultado = (|| -> windows::core::Result<()> {
            let documento = XmlDocument::new()?;
            documento.LoadXml(&HSTRING::from(xml(recado)))?;
            let notificacao = ToastNotification::CreateToastNotification(&documento)?;
            notificacao.SetTag(&HSTRING::from(&etiqueta))?;
            notificacao.SetGroup(&HSTRING::from(GRUPO))?;
            notificacao.SetData(&self.dados(recado)?)?;
            self.notificador.Show(&notificacao)
        })();
        resultado.is_ok()
    }

    /// Atualiza no lugar o recado com esta etiqueta — a barra anda, e nada reaparece.
    ///
    /// Devolve se havia o que atualizar: a pessoa pode ter fechado o recado, e aí não há.
    pub fn atualizar(&mut self, recado: &Recado, etiqueta: &str) -> bool {
        let Ok(dados) = self.dados(recado) else {
            return false;
        };
        self.notificador
            .UpdateWithTagAndGroup(&dados, &HSTRING::from(etiqueta), &HSTRING::from(GRUPO))
            .is_ok_and(|resultado| resultado == NotificationUpdateResult::Succeeded)
    }

    /// Os dados do recado, que preenchem os buracos do XML.
    fn dados(&mut self, recado: &Recado) -> windows::core::Result<NotificationData> {
        self.sequencia = self.sequencia.wrapping_add(1);
        let dados = NotificationData::new()?;
        let valores = dados.Values()?;
        valores.Insert(&HSTRING::from("titulo"), &HSTRING::from(&recado.titulo))?;
        valores.Insert(&HSTRING::from("corpo"), &HSTRING::from(&recado.corpo))?;
        let andamento = recado.andamento.unwrap_or(1.0).clamp(0.0, 1.0);
        valores.Insert(
            &HSTRING::from("andamento"),
            &HSTRING::from(format!("{andamento:.3}")),
        )?;
        dados.SetSequenceNumber(self.sequencia)?;
        Ok(dados)
    }
}

/// O XML do recado: com barra enquanto anda; com "Abrir a pasta" no que chegou.
fn xml(recado: &Recado) -> String {
    if recado.andamento.is_some() {
        return COM_ANDAMENTO.to_owned();
    }
    let (abrir, botao) = recado.pasta.as_deref().map_or_else(
        || (String::new(), String::new()),
        |pasta| {
            let uri = atributo(&uri_de_arquivo(Path::new(pasta)));
            (
                format!(r#" activationType="protocol" launch="{uri}""#),
                format!(
                    r#"<actions><action content="Abrir a pasta" activationType="protocol" arguments="{uri}"/></actions>"#
                ),
            )
        },
    );
    // Só o que deu errado faz som: o resto é retorno, e não interrupção.
    let som = if recado.tom == Tom::Problema {
        ""
    } else {
        r#"<audio silent="true"/>"#
    };
    format!(
        r#"<toast{abrir}><visual><binding template="ToastGeneric"><text>{{titulo}}</text><text>{{corpo}}</text></binding></visual>{botao}{som}</toast>"#
    )
}

/// Registra o nome e o ícone do InputRemote na central de notificações, para este usuário.
fn registrar() -> windows::core::Result<()> {
    let chave = windows_registry::CURRENT_USER
        .create(format!(r"Software\Classes\AppUserModelId\{AUMID}"))?;
    chave.set_string("DisplayName", "InputRemote")?;
    if let Some(icone) = gravar_icone() {
        chave.set_string("IconUri", icone.to_string_lossy().as_ref())?;
    }
    Ok(())
}

/// Grava o ícone onde a central consegue lê-lo. Sem ele, a central mostra um ícone genérico.
fn gravar_icone() -> Option<PathBuf> {
    let pasta = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("InputRemote");
    let icone = pasta.join("icone-notificacao.png");
    if std::fs::read(&icone).ok().as_deref() != Some(ICONE) {
        std::fs::create_dir_all(&pasta).ok()?;
        std::fs::write(&icone, ICONE).ok()?;
    }
    Some(icone)
}

/// O endereço `file:` de uma pasta, que o Windows abre no Explorer.
fn uri_de_arquivo(pasta: &Path) -> String {
    let caminho = pasta.to_string_lossy().replace('\\', "/");
    let mut uri = String::from("file:///");
    for c in caminho.chars() {
        match c {
            ' ' => uri.push_str("%20"),
            '%' => uri.push_str("%25"),
            '#' => uri.push_str("%23"),
            '?' => uri.push_str("%3F"),
            outro => uri.push(outro),
        }
    }
    uri
}

/// O texto pronto para ir dentro de um atributo do XML.
fn atributo(texto: &str) -> String {
    texto
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recado(tom: Tom, andamento: Option<f32>, pasta: Option<&str>) -> Recado {
        Recado {
            titulo: "<titulo & cia>".to_owned(),
            corpo: "corpo".to_owned(),
            tom,
            andamento,
            pasta: pasta.map(str::to_owned),
        }
    }

    #[test]
    fn o_texto_nunca_entra_no_xml() {
        for xml in [
            xml(&recado(Tom::Feito, None, None)),
            xml(&recado(Tom::Andamento, Some(0.5), None)),
        ] {
            assert!(!xml.contains("titulo &"), "{xml}");
            assert!(xml.contains("{titulo}"), "{xml}");
        }
    }

    #[test]
    fn o_que_chegou_abre_a_pasta_pelo_explorer() {
        let xml = xml(&recado(
            Tom::Feito,
            None,
            Some(r"C:\Users\Ana\Recebidos & cia\50% #1"),
        ));
        assert!(
            xml.contains(r#"launch="file:///C:/Users/Ana/Recebidos%20&amp;%20cia/50%25%20%231""#),
            "{xml}"
        );
        assert!(xml.contains("Abrir a pasta"), "{xml}");
    }

    #[test]
    fn so_o_problema_faz_som() {
        assert!(!xml(&recado(Tom::Problema, None, None)).contains("silent"));
        assert!(xml(&recado(Tom::Feito, None, None)).contains("silent"));
        assert!(xml(&recado(Tom::Andamento, Some(0.1), None)).contains("silent"));
    }
}
