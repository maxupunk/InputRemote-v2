//! O uso de uma placa de vídeo a partir do tempo que cada motor dela passou trabalhando. Puro: as
//! fontes de cada sistema leem os tempos, e a conta mora aqui.
//!
//! Cada motor (3D, cópia, vídeo…) conta o tempo em que esteve ocupado desde que a máquina ligou.
//! O uso de um motor é quanto esse tempo andou entre duas leituras, dividido pelo tempo que passou;
//! o da placa é o do motor mais ocupado — a mesma conta do Gerenciador de Tarefas, para os dois
//! dizerem o mesmo número.

/// O uso da placa em milésimos, entre os tempos de antes e os de agora de cada motor, na mesma
/// unidade de `intervalo`. Sem intervalo, ou com os motores mudando de número (o driver
/// reiniciou), não há conta: `None`.
#[must_use]
pub fn uso_dos_motores(antes: &[u64], agora: &[u64], intervalo: u64) -> Option<u16> {
    if intervalo == 0 || antes.len() != agora.len() {
        return None;
    }
    let mais_ocupado = antes
        .iter()
        .zip(agora)
        .map(|(antes, agora)| agora.saturating_sub(*antes))
        .max()
        .unwrap_or(0);
    let milesimos = u128::from(mais_ocupado) * 1000 / u128::from(intervalo);
    Some(u16::try_from(milesimos.min(1000)).unwrap_or(1000))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_uso_e_o_do_motor_mais_ocupado() {
        // Num segundo (10 milhões de 100 ns), o 3D trabalhou 0,35 s e o vídeo 0,1 s.
        let antes = [1_000, 50_000, 0];
        let agora = [1_000 + 3_500_000, 50_000 + 1_000_000, 0];
        assert_eq!(uso_dos_motores(&antes, &agora, 10_000_000), Some(350));
    }

    #[test]
    fn nunca_passa_de_cem_por_cento_e_sem_conta_nao_inventa() {
        assert_eq!(uso_dos_motores(&[0], &[20], 10), Some(1000));
        assert_eq!(uso_dos_motores(&[0], &[20], 0), None);
        assert_eq!(
            uso_dos_motores(&[0, 0], &[20], 10),
            None,
            "o driver mudou os motores"
        );
        assert_eq!(
            uso_dos_motores(&[30], &[20], 10),
            Some(0),
            "contador que voltou não é negativo"
        );
    }
}
