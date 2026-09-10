//! Descoberta na rede local por mDNS.
//!
//! Anuncia `_inputremote._udp.local.` e procura outros ([03, §10](../../../docs/03-protocolo.md)).
//! O anúncio traz id da máquina, nome e porta — e **nada mais**: sem usuário, sem chave, sem
//! código de pareamento, sem conteúdo de clipboard.
//!
//! Endereço manual continua sempre disponível, para redes que isolam clientes entre si; a
//! descoberta é uma conveniência, não a única porta de entrada.

use std::net::SocketAddr;
use std::time::Duration;

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

use crate::error::{NetError, Result};

/// O tipo de serviço mDNS do produto.
const SERVICE_TYPE: &str = "_inputremote._udp.local.";

/// Um computador que a descoberta encontrou.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Como o usuário reconhece este computador (o nome anunciado).
    pub label: String,
    /// O identificador da instalação, como veio no anúncio.
    pub machine: String,
    /// Onde alcançá-lo.
    pub addr: SocketAddr,
}

/// Um serviço de descoberta ativo.
pub struct Discovery {
    daemon: ServiceDaemon,
    instance: Option<String>,
}

impl core::fmt::Debug for Discovery {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Discovery")
            .field("instance", &self.instance)
            .finish_non_exhaustive()
    }
}

impl Discovery {
    /// Sobe o serviço de descoberta.
    ///
    /// # Errors
    ///
    /// [`NetError::Discovery`] se o daemon mDNS não puder ser criado.
    pub fn new() -> Result<Self> {
        let daemon = ServiceDaemon::new().map_err(|e| NetError::Discovery(e.to_string()))?;
        Ok(Self {
            daemon,
            instance: None,
        })
    }

    /// Anuncia esta máquina na rede.
    ///
    /// # Errors
    ///
    /// [`NetError::Discovery`] se o registro falhar.
    pub fn advertise(&mut self, machine: &str, name: &str, port: u16) -> Result<()> {
        let instance = format!("{machine}.{SERVICE_TYPE}");
        let host = format!("{machine}.local.");
        let properties = [("maquina", machine), ("nome", name)];
        let info = ServiceInfo::new(SERVICE_TYPE, machine, &host, (), port, &properties[..])
            .map_err(|e| NetError::Discovery(e.to_string()))?
            .enable_addr_auto();
        self.daemon
            .register(info)
            .map_err(|e| NetError::Discovery(e.to_string()))?;
        self.instance = Some(instance);
        Ok(())
    }

    /// Procura computadores por `duration`, devolvendo o que encontrar.
    ///
    /// # Errors
    ///
    /// [`NetError::Discovery`] se a busca não puder ser iniciada.
    pub async fn discover(&self, duration: Duration) -> Result<Vec<Candidate>> {
        let receiver = self
            .daemon
            .browse(SERVICE_TYPE)
            .map_err(|e| NetError::Discovery(e.to_string()))?;
        let mut found = Vec::new();
        let deadline = tokio::time::Instant::now() + duration;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            let event = tokio::time::timeout(remaining, receiver.recv_async()).await;
            match event {
                Ok(Ok(ServiceEvent::ServiceResolved(info))) => {
                    if let Some(candidate) = candidate_from(&info)
                        && !found.contains(&candidate)
                    {
                        found.push(candidate);
                    }
                }
                Ok(Ok(_)) => {}
                Ok(Err(_)) | Err(_) => break,
            }
        }
        let _ = self.daemon.stop_browse(SERVICE_TYPE);
        Ok(found)
    }
}

/// Extrai um candidato de um serviço resolvido, se ele tiver um endereço utilizável.
fn candidate_from(info: &ServiceInfo) -> Option<Candidate> {
    let addr = info.get_addresses().iter().next().copied()?;
    let port = info.get_port();
    let machine = info
        .get_property_val_str("maquina")
        .unwrap_or_else(|| info.get_fullname())
        .to_owned();
    let label = info
        .get_property_val_str("nome")
        .unwrap_or(&machine)
        .to_owned();
    Some(Candidate {
        label,
        machine,
        addr: SocketAddr::new(addr, port),
    })
}
