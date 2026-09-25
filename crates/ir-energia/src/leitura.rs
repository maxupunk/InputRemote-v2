//! A leitura do que as ferramentas do sistema respondem. Pura, e por isso testada nos dois sistemas.

use crate::Economia;

/// O que `iw dev <placa> get power_save` diz: `Power save: on` ou `Power save: off`.
#[must_use]
pub fn economia_do_iw(saida: &str) -> Economia {
    let resposta = saida
        .lines()
        .find_map(|linha| linha.trim().strip_prefix("Power save:"))
        .map(str::trim);
    match resposta {
        Some("on") => Economia::Ligada,
        Some("off") => Economia::Desligada,
        _ => Economia::Desconhecida,
    }
}

/// O que `powercfg /q` diz da configuração "modo de economia de energia" do adaptador sem fio.
///
/// A saída muda de idioma com o Windows, então não se lê o texto: as duas únicas contagens em
/// hexadecimal (`0x…`) são o índice na tomada e o índice na bateria, nessa ordem. Zero é
/// "desempenho máximo".
#[must_use]
pub fn economia_do_powercfg(saida: &str) -> Economia {
    let indices: Vec<u32> = ir_processo::ferramenta::numeros_hex(saida).collect();
    match indices.as_slice() {
        [tomada, bateria] => match (*tomada, *bateria) {
            (0, 0) => Economia::Desligada,
            (0, _) => Economia::SoNaBateria,
            _ => Economia::Ligada,
        },
        _ => Economia::Desconhecida,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_iw_diz_ligada_ou_desligada() {
        assert_eq!(economia_do_iw("Power save: on\n"), Economia::Ligada);
        assert_eq!(economia_do_iw("\tPower save: off\n"), Economia::Desligada);
        assert_eq!(
            economia_do_iw("command failed: No such device (-19)"),
            Economia::Desconhecida
        );
    }

    /// A saída do Windows em português, como a bancada devolveu.
    const POWERCFG_PT: &str = "\
GUID do Esquema de Energia: 381b4222-f694-41f0-9685-ff5bb260df2e  (Equilibrado)
  GUID do Subgrupo: 19cbb8fa-5279-450e-9fac-8a3d5fedd0c1  (Configurações do Adaptador sem Fio)
    GUID da Configuração de Energia: 12bbebe6-58d6-4636-95bb-3217ef867c1a  (Modo de Economia)
      Índice de Configurações Possíveis: 000
      Nome Amigável de Configuração Possível: Desempenho Máximo
      Índice de Configurações Possíveis: 003
      Nome Amigável de Configuração Possível: Economia de Energia Máxima
    Índice de Configurações de Correntes Alternadas Atuais: 0x00000000
    Índice de Configurações de Correntes Contínuas Atuais: 0x00000002
";

    #[test]
    fn o_powercfg_em_portugues_diz_so_na_bateria() {
        assert_eq!(economia_do_powercfg(POWERCFG_PT), Economia::SoNaBateria);
    }

    #[test]
    fn o_powercfg_em_ingles_e_lido_do_mesmo_jeito() {
        let ligada = "Current AC Power Setting Index: 0x00000003\n\
                      Current DC Power Setting Index: 0x00000003\n";
        assert_eq!(economia_do_powercfg(ligada), Economia::Ligada);
        let desligada = "Current AC Power Setting Index: 0x00000000\n\
                         Current DC Power Setting Index: 0x00000000\n";
        assert_eq!(economia_do_powercfg(desligada), Economia::Desligada);
    }

    #[test]
    fn um_powercfg_inesperado_nao_inventa_resposta() {
        assert_eq!(
            economia_do_powercfg("Parâmetro inválido"),
            Economia::Desconhecida
        );
    }
}
