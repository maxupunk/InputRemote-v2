//! O andamento da cópia no ícone do InputRemote no dock.
//!
//! O GNOME não desenha barra na notificação, mas o dock sabe desenhar barra sobre o ícone de um
//! programa: o Dash to Dock, o Ubuntu Dock, o Dash to Panel e o KDE atendem o sinal
//! `com.canonical.Unity.LauncherEntry.Update`, o mesmo que o Firefox e o Nautilus mandam durante um
//! download ou uma cópia. Onde ninguém atende — o dock padrão do GNOME —, o sinal se perde, sem
//! custo.
//!
//! A conexão com o barramento fica de pé enquanto o mostrador viver: o dock apaga a barra de quem
//! sai do barramento. Por isso não serve um `gdbus emit` por atualização.

use std::collections::HashMap;

use zbus::zvariant::Value;

/// O programa, pelo arquivo `.desktop` instalado: é assim que o dock acha o ícone.
const PROGRAMA: &str = "application://inputremote.desktop";
/// De onde o sinal sai. Qualquer caminho serve; o dock olha o `PROGRAMA`.
const CAMINHO: &str = "/com/inputremote/doca";
const INTERFACE: &str = "com.canonical.Unity.LauncherEntry";

/// A barra no ícone do dock.
#[derive(Debug)]
pub struct Doca {
    conexao: zbus::blocking::Connection,
    /// A última porcentagem mostrada; `None` com a barra escondida.
    ultima: Option<u8>,
}

impl Doca {
    /// Ligada ao barramento da sessão. `None` sem barramento — um console, um contêiner.
    #[must_use]
    pub fn abrir() -> Option<Self> {
        let conexao = zbus::blocking::Connection::session().ok()?;
        Some(Self {
            conexao,
            ultima: None,
        })
    }

    /// O andamento, de 0 a 1; `None` esconde a barra. Só manda quando a porcentagem muda.
    pub fn andamento(&mut self, andamento: Option<f32>) {
        let por_cento = andamento.map(super::porcento);
        if por_cento == self.ultima {
            return;
        }
        self.ultima = por_cento;
        let mut propriedades: HashMap<&str, Value<'_>> = HashMap::new();
        propriedades.insert(
            "progress",
            Value::from(f64::from(por_cento.unwrap_or(0)) / 100.0),
        );
        propriedades.insert("progress-visible", Value::from(por_cento.is_some()));
        if let Err(erro) = self.conexao.emit_signal(
            None::<&str>,
            CAMINHO,
            INTERFACE,
            "Update",
            &(PROGRAMA, propriedades),
        ) {
            tracing::debug!(%erro, "o andamento não foi ao dock");
        }
    }
}
