//! O fluxo da economia de energia visto de fora: o que a janela vê e o que o botão faz.

use ir_energia::Economia;
use ir_ipc::{EconomiaDoWifi, Falha, Resposta};
use ir_proto::message::NetworkPowerSaving;
use ir_session::Role;

use crate::actor::bancada::Bancada;

#[test]
fn a_placa_daqui_aparece_no_estado_e_desconhecida_nao_apaga() {
    let mut bancada = Bancada::nova(Role::Server);
    assert_eq!(bancada.daemon.estado().economia_aqui, None);

    bancada.daemon.on_economia(Economia::Ligada);
    assert_eq!(
        bancada.daemon.estado().economia_aqui,
        Some(EconomiaDoWifi::Ligada)
    );

    // Uma verificação que não deu resposta não é placa nova.
    bancada.daemon.on_economia(Economia::Desconhecida);
    assert_eq!(
        bancada.daemon.estado().economia_aqui,
        Some(EconomiaDoWifi::Ligada)
    );

    bancada.daemon.on_economia(Economia::Desligada);
    assert_eq!(bancada.daemon.estado().economia_aqui, None);
}

#[test]
fn o_que_o_par_contou_aparece_e_vira_aviso_do_par() {
    let mut bancada = Bancada::nova(Role::Server);
    bancada
        .daemon
        .on_economia_do_par(Some(NetworkPowerSaving::On));

    let estado = bancada.daemon.estado();
    assert_eq!(estado.economia_no_par, Some(EconomiaDoWifi::Ligada));
    assert!(estado.aviso_de_rede().is_some_and(|aviso| aviso.no_par));
}

#[test]
fn sem_sessao_pedir_ao_par_e_recusado_com_motivo() {
    let mut bancada = Bancada::nova(Role::Server);
    assert_eq!(
        bancada.daemon.desligar_economia(true),
        Resposta::Falha(Falha::ForaDeContexto)
    );
}
