use super::*;

/// As duas pontas e o que cada uma ainda não entregou à outra.
struct Bancada {
    painel: Conversa,
    computador: Conversa,
    do_painel: Vec<u8>,
    do_computador: Vec<u8>,
    eventos_painel: Vec<Evento>,
    eventos_computador: Vec<Evento>,
}

impl Bancada {
    fn nova(modo: ConnectMode, computador: &Identity, politica: Politica) -> Self {
        let painel_id = Identity::generate();
        Self::com(&painel_id, modo, computador, politica)
    }

    fn com(
        painel_id: &Identity,
        modo: ConnectMode,
        computador: &Identity,
        politica: Politica,
    ) -> Self {
        let (painel, primeiros) = Conversa::discar(painel_id, modo).unwrap();
        Self {
            painel,
            computador: Conversa::atender(computador.clone(), politica),
            do_painel: primeiros,
            do_computador: Vec::new(),
            eventos_painel: Vec::new(),
            eventos_computador: Vec::new(),
        }
    }

    /// Entrega o pendente nos dois sentidos até ninguém ter mais nada a mandar.
    fn rodar(&mut self) -> Result<(), Falha> {
        for _ in 0..8 {
            let para_computador = core::mem::take(&mut self.do_painel);
            if !para_computador.is_empty() {
                let eventos = self.computador.alimentar(&para_computador)?;
                self.do_computador.extend(bytes_de(&eventos));
                self.eventos_computador.extend(eventos);
            }
            let para_painel = core::mem::take(&mut self.do_computador);
            if !para_painel.is_empty() {
                let eventos = self.painel.alimentar(&para_painel)?;
                self.do_painel.extend(bytes_de(&eventos));
                self.eventos_painel.extend(eventos);
            }
        }
        Ok(())
    }

    fn confirmar(&mut self, painel: bool, computador: bool) -> Result<(), Falha> {
        let eventos = self.computador.confirmar(computador)?;
        self.do_computador.extend(bytes_de(&eventos));
        self.eventos_computador.extend(eventos);
        let eventos = self.painel.confirmar(painel)?;
        self.do_painel.extend(bytes_de(&eventos));
        self.eventos_painel.extend(eventos);
        self.rodar()
    }
}

fn bytes_de(eventos: &[Evento]) -> Vec<u8> {
    eventos
        .iter()
        .filter_map(|e| match e {
            Evento::Enviar(bytes) => Some(bytes.clone()),
            _ => None,
        })
        .flatten()
        .collect()
}

fn codigo(eventos: &[Evento]) -> Option<[u8; 6]> {
    eventos.iter().find_map(|e| match e {
        Evento::Codigo { digitos, .. } => Some(*digitos),
        _ => None,
    })
}

fn abriu(eventos: &[Evento]) -> Option<(PublicKey, bool)> {
    eventos.iter().find_map(|e| match e {
        Evento::Aberta { par, pareou } => Some((*par, *pareou)),
        _ => None,
    })
}

fn aberta_para_parear() -> Politica {
    Politica {
        conhecidos: Vec::new(),
        aceita_pareamento: true,
    }
}

#[test]
fn parear_mostra_o_mesmo_codigo_e_so_abre_com_as_duas_confirmacoes() {
    let computador = Identity::generate();
    let mut bancada = Bancada::nova(ConnectMode::Pair, &computador, aberta_para_parear());
    bancada.rodar().unwrap();

    let no_painel = codigo(&bancada.eventos_painel).expect("o painel mostra o código");
    let no_computador = codigo(&bancada.eventos_computador).expect("o computador também");
    assert_eq!(no_painel, no_computador);
    assert!(!bancada.painel.aberta() && !bancada.computador.aberta());

    // Só o computador confirma: ninguém abre ainda.
    let eventos = bancada.computador.confirmar(true).unwrap();
    bancada.do_computador.extend(bytes_de(&eventos));
    bancada.rodar().unwrap();
    assert!(!bancada.computador.aberta());

    let eventos = bancada.painel.confirmar(true).unwrap();
    bancada.do_painel.extend(bytes_de(&eventos));
    bancada.eventos_painel.extend(eventos);
    bancada.rodar().unwrap();

    assert!(bancada.painel.aberta() && bancada.computador.aberta());
    assert_eq!(
        abriu(&bancada.eventos_painel),
        Some((computador.public(), true))
    );
}

#[test]
fn a_recusa_do_computador_chega_ao_painel() {
    let computador = Identity::generate();
    let mut bancada = Bancada::nova(ConnectMode::Pair, &computador, aberta_para_parear());
    bancada.rodar().unwrap();

    let eventos = bancada.computador.confirmar(false).unwrap();
    assert!(bancada.computador.encerrada());
    assert_eq!(
        bancada.painel.alimentar(&bytes_de(&eventos)),
        Err(Falha::CodigoRecusado)
    );
    assert!(bancada.painel.encerrada());
}

#[test]
fn a_recusa_do_painel_chega_ao_computador_mesmo_depois_de_ele_confirmar() {
    let computador = Identity::generate();
    let mut bancada = Bancada::nova(ConnectMode::Pair, &computador, aberta_para_parear());
    bancada.rodar().unwrap();
    bancada.computador.confirmar(true).unwrap();

    let eventos = bancada.painel.confirmar(false).unwrap();
    assert_eq!(
        bancada.computador.alimentar(&bytes_de(&eventos)),
        Err(Falha::CodigoRecusado)
    );
    assert!(!bancada.computador.aberta());
}

#[test]
fn com_a_janela_fechada_um_painel_novo_e_recusado() {
    let computador = Identity::generate();
    let mut bancada = Bancada::nova(ConnectMode::Pair, &computador, Politica::default());
    assert_eq!(bancada.rodar(), Err(Falha::Recusado));
}

#[test]
fn um_painel_conhecido_volta_sem_codigo() {
    let computador = Identity::generate();
    let painel = Identity::generate();
    let politica = Politica {
        conhecidos: vec![painel.public()],
        aceita_pareamento: false,
    };
    let mut bancada = Bancada::com(
        &painel,
        ConnectMode::Reconnect(computador.public()),
        &computador,
        politica,
    );
    bancada.rodar().unwrap();
    assert!(codigo(&bancada.eventos_computador).is_none());
    assert_eq!(
        abriu(&bancada.eventos_computador),
        Some((painel.public(), false))
    );
    assert!(bancada.painel.aberta());
}

#[test]
fn um_painel_desconhecido_nao_volta() {
    let computador = Identity::generate();
    let politica = Politica {
        conhecidos: vec![Identity::generate().public()],
        aceita_pareamento: true,
    };
    let mut bancada = Bancada::nova(
        ConnectMode::Reconnect(computador.public()),
        &computador,
        politica,
    );
    assert_eq!(bancada.rodar(), Err(Falha::Recusado));
}

#[test]
fn voltar_para_a_chave_errada_nao_fecha() {
    let computador = Identity::generate();
    let impostor = Identity::generate();
    let mut bancada = Bancada::nova(
        ConnectMode::Reconnect(computador.public()),
        &impostor,
        Politica {
            conhecidos: Vec::new(),
            aceita_pareamento: false,
        },
    );
    assert!(bancada.rodar().is_err());
    assert!(!bancada.painel.aberta());
}

#[test]
fn aberta_as_mensagens_vao_e_voltam_mesmo_picadas() {
    let computador = Identity::generate();
    let mut bancada = Bancada::nova(ConnectMode::Pair, &computador, aberta_para_parear());
    bancada.rodar().unwrap();
    bancada.confirmar(true, true).unwrap();

    let bytes = bancada.painel.enviar(b"assinar").unwrap();
    // O TCP entrega como quer: um byte de cada vez.
    let mut recebidas = Vec::new();
    for byte in &bytes {
        recebidas.extend(bancada.computador.alimentar(&[*byte]).unwrap());
    }
    assert_eq!(recebidas, vec![Evento::Mensagem(b"assinar".to_vec())]);

    let volta = bancada.computador.enviar(b"quadro").unwrap();
    let mut duas = volta.clone();
    duas.extend(bancada.computador.enviar(b"outro").unwrap());
    assert_eq!(
        bancada.painel.alimentar(&duas).unwrap(),
        vec![
            Evento::Mensagem(b"quadro".to_vec()),
            Evento::Mensagem(b"outro".to_vec())
        ]
    );
}

#[test]
fn antes_de_abrir_nao_se_manda_mensagem() {
    let computador = Identity::generate();
    let mut bancada = Bancada::nova(ConnectMode::Pair, &computador, aberta_para_parear());
    bancada.rodar().unwrap();
    assert_eq!(bancada.painel.enviar(b"x"), Err(Falha::Encerrada));
}

#[test]
fn lixo_no_primeiro_corpo_e_malformado() {
    let mut computador = Conversa::atender(Identity::generate(), aberta_para_parear());
    let lixo = enquadrar(&[9, 1, 2, 3]).unwrap();
    assert_eq!(computador.alimentar(&lixo), Err(Falha::Malformado));
}

#[test]
fn um_anuncio_absurdo_de_tamanho_e_recusado_antes_de_alocar() {
    let mut computador = Conversa::atender(Identity::generate(), aberta_para_parear());
    assert_eq!(computador.alimentar(&[0xff, 0xff]), Err(Falha::Malformado));
}

#[test]
fn quem_disca_nao_troca_chaves() {
    let painel = Identity::generate();
    assert!(Conversa::discar(&painel, ConnectMode::Rekey(painel.public())).is_err());
}
