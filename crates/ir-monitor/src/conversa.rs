//! A conversa cifrada entre o painel e o computador, sem E/S.
//!
//! Recebe os bytes que o socket entregou e devolve [`Evento`]s: bytes a mandar, o código a
//! conferir, a conversa aberta, uma mensagem. Quem move bytes é a ponta — o `tokio` do serviço, a
//! linha bloqueante do Android —, e as duas usam esta mesma máquina.
//!
//! # No fio
//!
//! Sobre TCP, cada corpo leva um prefixo `u16` *little-endian* ([`enlace::enquadrar`]). O primeiro
//! corpo é `[modo][mensagem Noise]`, e o modo escolhe o padrão: `XX` para parear, `IK` com a chave
//! do computador fixada para voltar. Os demais corpos do aperto de mão são só a mensagem Noise.
//! Depois dele, cada corpo é um quadro cifrado, com o contador **contado** pelas duas pontas, como
//! no rádio e no canal de dados ([`ContadorImplicito`]), e com o byte de espécie por dentro.
//!
//! # As duas confirmações
//!
//! Pareando, nada de mensagem trafega antes de as **duas** telas confirmarem o código
//! ([`Confirmacao`]): a do computador, pela janela, e a do painel, pelo botão no aparelho.

use ir_crypto::enlace::{
    self, Confirmacao, ConnectMode, ContadorImplicito, Desenquadrador, Desfecho, Kind, Mode,
};
use ir_crypto::{Handshake, Identity, PublicKey, Transport};

/// Bytes do prefixo de tamanho.
const PREFIXO: usize = 2;

/// O maior corpo, cifrado. Um quadro com dois computadores e todas as temperaturas fica abaixo de
/// 400 B; 8 KiB é folga que ainda impede um anúncio absurdo de alocar memória à vontade.
pub const TETO: usize = 8 * 1024;

/// O que esta ponta aceita de quem atende. Só o computador tem política: o painel disca.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Politica {
    /// Os painéis já pareados, que podem voltar com `IK`.
    pub conhecidos: Vec<PublicKey>,
    /// Se a janela de pareamento está aberta, e um painel novo pode parear.
    pub aceita_pareamento: bool,
}

/// O que a conversa pede à ponta, ou conta a ela.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Evento {
    /// Mande estes bytes pelo socket, nesta ordem.
    Enviar(Vec<u8>),
    /// Pareando: mostre o código e pergunte se é o mesmo da outra tela.
    Codigo {
        /// Os seis dígitos, de 0 a 9.
        digitos: [u8; 6],
        /// A chave de quem está do outro lado — a que vai ser gravada se o código conferir.
        par: PublicKey,
    },
    /// A conversa abriu: daqui em diante, mensagens.
    Aberta {
        /// A chave do outro lado.
        par: PublicKey,
        /// Se abriu por um pareamento agora (e a chave precisa ser gravada), e não por uma volta.
        pareou: bool,
    },
    /// Uma mensagem do outro lado, já decifrada.
    Mensagem(Vec<u8>),
}

/// Por que a conversa acabou.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Falha {
    /// Bytes fora do formato, ou fora de hora.
    #[error("o outro lado mandou algo fora do formato")]
    Malformado,
    /// O Noise recusou: chave errada, mensagem adulterada, contagens divergentes.
    #[error("a criptografia recusou a conversa")]
    Cripto,
    /// Um painel desconhecido, ou um pareamento com a janela fechada.
    #[error("este computador não aceita este painel agora")]
    Recusado,
    /// Alguém disse que o código não confere.
    #[error("o código de pareamento não foi confirmado")]
    CodigoRecusado,
    /// A conversa já tinha acabado.
    #[error("a conversa já acabou")]
    Encerrada,
}

impl From<ir_crypto::CryptoError> for Falha {
    fn from(_: ir_crypto::CryptoError) -> Self {
        Self::Cripto
    }
}

impl From<enlace::Excesso> for Falha {
    fn from(_: enlace::Excesso) -> Self {
        Self::Malformado
    }
}

enum Fase {
    /// Quem atende, antes do primeiro corpo: ainda não sabe o modo.
    Esperando(Identity, Politica),
    Apertando(Box<Handshake>, Option<Politica>),
    Conferindo(Conferencia),
    Aberta(Cifra),
    Encerrada,
}

/// Pareando, entre o código na tela e as duas respostas.
struct Conferencia {
    cifra: Cifra,
    confirmacao: Confirmacao,
    par: PublicKey,
}

/// O transporte e a contagem de quem recebe.
struct Cifra {
    transporte: Transport,
    contador: ContadorImplicito,
}

/// A conversa de uma ponta.
pub struct Conversa {
    fase: Fase,
    entrada: Desenquadrador<PREFIXO, TETO>,
}

impl core::fmt::Debug for Conversa {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let fase = match self.fase {
            Fase::Esperando(..) => "esperando",
            Fase::Apertando(..) => "apertando a mão",
            Fase::Conferindo(..) => "conferindo o código",
            Fase::Aberta(_) => "aberta",
            Fase::Encerrada => "encerrada",
        };
        f.debug_struct("Conversa")
            .field("fase", &fase)
            .finish_non_exhaustive()
    }
}

impl Conversa {
    /// O painel disca: pareia do zero ([`ConnectMode::Pair`]) ou volta a um computador conhecido
    /// ([`ConnectMode::Reconnect`] com a chave dele). Devolve a conversa e os bytes a mandar.
    ///
    /// # Errors
    ///
    /// [`Falha::Cripto`] se o Noise não montar; [`Falha::Malformado`] para [`ConnectMode::Rekey`],
    /// que este enlace não usa.
    pub fn discar(identidade: &Identity, modo: ConnectMode) -> Result<(Self, Vec<u8>), Falha> {
        if matches!(modo, ConnectMode::Rekey(_)) {
            return Err(Falha::Malformado);
        }
        let mut aperto = modo.iniciar(identidade)?;
        let primeira = aperto.write_message()?;
        let corpo = enlace::corpo_de_handshake(modo.modo(), &primeira);
        let conversa = Self {
            fase: Fase::Apertando(Box::new(aperto), None),
            entrada: Desenquadrador::novo(),
        };
        Ok((conversa, enquadrar(&corpo)?))
    }

    /// O computador atende, com a política de agora.
    #[must_use]
    pub fn atender(identidade: Identity, politica: Politica) -> Self {
        Self {
            fase: Fase::Esperando(identidade, politica),
            entrada: Desenquadrador::novo(),
        }
    }

    /// Se a conversa já abriu.
    #[must_use]
    pub const fn aberta(&self) -> bool {
        matches!(self.fase, Fase::Aberta(_))
    }

    /// Se a conversa acabou: esta tela recusou o código, ou houve [`Falha`].
    #[must_use]
    pub const fn encerrada(&self) -> bool {
        matches!(self.fase, Fase::Encerrada)
    }

    /// Os bytes que o socket entregou, do tamanho que vieram.
    ///
    /// # Errors
    ///
    /// Qualquer [`Falha`]: a conversa acaba, e a ponta fecha o socket.
    pub fn alimentar(&mut self, bytes: &[u8]) -> Result<Vec<Evento>, Falha> {
        self.entrada.alimentar(bytes);
        let mut eventos = Vec::new();
        while let Some(corpo) = self.entrada.proximo()? {
            if let Err(falha) = self.receber(&corpo, &mut eventos) {
                self.fase = Fase::Encerrada;
                return Err(falha);
            }
        }
        Ok(eventos)
    }

    /// A resposta desta tela ao código: `true` se confere.
    ///
    /// A resposta vai **sempre** ao outro lado, a recusa também, para ele sair da tela de
    /// comparação em vez de esperar o próprio prazo. Recusando, a conversa fica
    /// [encerrada](Self::encerrada) depois desses bytes: mande-os e feche.
    ///
    /// # Errors
    ///
    /// [`Falha::Malformado`] fora da conferência; [`Falha::Cripto`] se o Noise recusar.
    pub fn confirmar(&mut self, conferiu: bool) -> Result<Vec<Evento>, Falha> {
        let Fase::Conferindo(mut conferencia) = core::mem::replace(&mut self.fase, Fase::Encerrada)
        else {
            return Err(Falha::Malformado);
        };
        let (especie, desfecho) = conferencia.confirmacao.local(conferiu);
        let mut eventos = vec![Evento::Enviar(conferencia.cifra.selar(especie, &[])?)];
        match desfecho {
            Desfecho::Derrubar => {}
            Desfecho::Esperar => self.fase = Fase::Conferindo(conferencia),
            Desfecho::Promover => {
                eventos.push(Evento::Aberta {
                    par: conferencia.par,
                    pareou: true,
                });
                self.fase = Fase::Aberta(conferencia.cifra);
            }
        }
        Ok(eventos)
    }

    /// Uma mensagem para o outro lado, pronta para o socket.
    ///
    /// # Errors
    ///
    /// [`Falha::Encerrada`] antes de a conversa abrir ou depois de acabar; [`Falha::Cripto`] se o
    /// Noise recusar.
    pub fn enviar(&mut self, conteudo: &[u8]) -> Result<Vec<u8>, Falha> {
        match &mut self.fase {
            Fase::Aberta(cifra) => cifra.selar(Kind::SessionFrame, conteudo),
            _ => Err(Falha::Encerrada),
        }
    }

    fn receber(&mut self, corpo: &[u8], eventos: &mut Vec<Evento>) -> Result<(), Falha> {
        match core::mem::replace(&mut self.fase, Fase::Encerrada) {
            Fase::Esperando(identidade, politica) => {
                self.primeiro_corpo(&identidade, politica, corpo, eventos)
            }
            Fase::Apertando(aperto, politica) => self.apertar(*aperto, politica, corpo, eventos),
            Fase::Conferindo(conferencia) => self.conferir(conferencia, corpo, eventos),
            Fase::Aberta(mut cifra) => {
                let (especie, conteudo) = cifra.abrir(corpo)?;
                if especie != Kind::SessionFrame {
                    return Err(Falha::Malformado);
                }
                eventos.push(Evento::Mensagem(conteudo));
                self.fase = Fase::Aberta(cifra);
                Ok(())
            }
            Fase::Encerrada => Err(Falha::Encerrada),
        }
    }

    fn primeiro_corpo(
        &mut self,
        identidade: &Identity,
        politica: Politica,
        corpo: &[u8],
        eventos: &mut Vec<Evento>,
    ) -> Result<(), Falha> {
        let (modo, mensagem) = enlace::ler_handshake(corpo).ok_or(Falha::Malformado)?;
        let mut aperto = match modo {
            Mode::Pair if politica.aceita_pareamento => Handshake::pair_responder(identidade)?,
            Mode::Pair => return Err(Falha::Recusado),
            Mode::Reconnect => Handshake::reconnect_responder(identidade)?,
            Mode::Rekey => return Err(Falha::Malformado),
        };
        aperto.read_message(mensagem)?;
        self.avancar(aperto, Some(politica), eventos)
    }

    fn apertar(
        &mut self,
        mut aperto: Handshake,
        politica: Option<Politica>,
        corpo: &[u8],
        eventos: &mut Vec<Evento>,
    ) -> Result<(), Falha> {
        aperto.read_message(corpo)?;
        self.avancar(aperto, politica, eventos)
    }

    /// Escreve o que for a vez desta ponta e, se o aperto terminou, conclui.
    fn avancar(
        &mut self,
        mut aperto: Handshake,
        politica: Option<Politica>,
        eventos: &mut Vec<Evento>,
    ) -> Result<(), Falha> {
        if aperto.is_my_turn() {
            let mensagem = aperto.write_message()?;
            eventos.push(Evento::Enviar(enquadrar(&mensagem)?));
        }
        if !aperto.is_finished() {
            self.fase = Fase::Apertando(Box::new(aperto), politica);
            return Ok(());
        }
        let feito = enlace::concluir(aperto)?;
        let cifra = Cifra {
            transporte: feito.transport,
            contador: ContadorImplicito::novo(),
        };
        let par = feito.peer_static;
        if let Some(digitos) = feito.code {
            eventos.push(Evento::Codigo { digitos, par });
            self.fase = Fase::Conferindo(Conferencia {
                cifra,
                confirmacao: Confirmacao::default(),
                par,
            });
            return Ok(());
        }
        // Na volta, quem atende confere a chave apresentada contra as dos pareados. Quem disca já a
        // fixou no padrão `IK`: se fosse outra, o aperto nem fecharia.
        if politica.is_some_and(|p| !p.conhecidos.contains(&par)) {
            return Err(Falha::Recusado);
        }
        eventos.push(Evento::Aberta { par, pareou: false });
        self.fase = Fase::Aberta(cifra);
        Ok(())
    }

    fn conferir(
        &mut self,
        mut conferencia: Conferencia,
        corpo: &[u8],
        eventos: &mut Vec<Evento>,
    ) -> Result<(), Falha> {
        let (especie, _) = conferencia.cifra.abrir(corpo)?;
        match especie {
            Kind::PairReject => Err(Falha::CodigoRecusado),
            Kind::SessionFrame => Err(Falha::Malformado),
            Kind::PairConfirm => {
                if conferencia.confirmacao.do_par() == Desfecho::Promover {
                    eventos.push(Evento::Aberta {
                        par: conferencia.par,
                        pareou: true,
                    });
                    self.fase = Fase::Aberta(conferencia.cifra);
                } else {
                    self.fase = Fase::Conferindo(conferencia);
                }
                Ok(())
            }
        }
    }
}

impl Cifra {
    fn selar(&mut self, especie: Kind, conteudo: &[u8]) -> Result<Vec<u8>, Falha> {
        let (_, cifrado) = self
            .transporte
            .seal(&enlace::embrulhar(especie, conteudo))?;
        enquadrar(&cifrado)
    }

    fn abrir(&mut self, corpo: &[u8]) -> Result<(Kind, Vec<u8>), Falha> {
        let transporte = &mut self.transporte;
        let claro = self
            .contador
            .abrir(|contador| transporte.open(contador, corpo))?;
        let (especie, conteudo) = enlace::desembrulhar(&claro).ok_or(Falha::Malformado)?;
        Ok((especie, conteudo.to_vec()))
    }
}

fn enquadrar(corpo: &[u8]) -> Result<Vec<u8>, Falha> {
    Ok(enlace::enquadrar::<PREFIXO, TETO>(corpo)?)
}

#[cfg(test)]
mod tests;
