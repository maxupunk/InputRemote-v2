//! O endereço de um rádio Bluetooth, e o canal RFCOMM do produto.
//!
//! Parte pura: nenhum socket, nenhuma pilha de sistema. As conversões para as representações de
//! cada plataforma ficam nos backends, que são quem conhece o formato delas.

use core::fmt;
use core::str::FromStr;

/// O canal RFCOMM em que o InputRemote atende.
///
/// Fixo e combinado entre as duas pontas, em vez de descoberto por SDP. O número está na faixa
/// livre (1–30) e fora dos canais que os perfis comuns tomam primeiro. O porquê da escolha, e o
/// que se perde com ela, estão em
/// [ADR-0009](../../../docs/adr/0009-canal-rfcomm-fixo-sem-sdp.md).
pub const CANAL: u8 = 23;

/// Quantos octetos tem um endereço Bluetooth.
const OCTETOS: usize = 6;

/// O endereço de 48 bits de um rádio Bluetooth.
///
/// Guardado na ordem em que se **lê e se escreve** — `AA:BB:CC:DD:EE:FF` é `[0xAA, .., 0xFF]`.
/// As pilhas discordam da ordem interna (o Windows usa um inteiro de 64 bits), e concentrar a
/// conversão aqui impede que a ordem de bytes de uma delas vaze para o resto do produto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BdAddr(pub [u8; OCTETOS]);

impl BdAddr {
    /// O endereço nulo, que nenhuma máquina tem.
    ///
    /// No Linux é o curinga de "qualquer adaptador local" ao vincular um socket.
    pub const NULO: Self = Self([0; OCTETOS]);

    /// Os seis octetos, na ordem de leitura.
    #[must_use]
    pub const fn bytes(self) -> [u8; OCTETOS] {
        self.0
    }

    /// Se é o endereço nulo.
    #[must_use]
    pub fn e_nulo(self) -> bool {
        self == Self::NULO
    }

    /// O endereço como o inteiro de 64 bits que o Winsock usa (`BTH_ADDR`).
    ///
    /// O octeto que se lê primeiro é o mais significativo do inteiro, e os dois bytes de cima
    /// ficam em zero.
    #[must_use]
    pub const fn para_u64(self) -> u64 {
        // Octetos do mais significativo ao menos, como se lê o endereço da esquerda para a
        // direita.
        let [o5, o4, o3, o2, o1, o0] = self.0;
        ((o5 as u64) << 40)
            | ((o4 as u64) << 32)
            | ((o3 as u64) << 24)
            | ((o2 as u64) << 16)
            | ((o1 as u64) << 8)
            | (o0 as u64)
    }

    /// O endereço a partir do inteiro de 64 bits do Winsock.
    ///
    /// Os 16 bits de cima são ignorados: um `BTH_ADDR` só carrega 48.
    #[must_use]
    pub const fn de_u64(valor: u64) -> Self {
        Self([
            ((valor >> 40) & 0xFF) as u8,
            ((valor >> 32) & 0xFF) as u8,
            ((valor >> 24) & 0xFF) as u8,
            ((valor >> 16) & 0xFF) as u8,
            ((valor >> 8) & 0xFF) as u8,
            (valor & 0xFF) as u8,
        ])
    }
}

impl fmt::Display for BdAddr {
    fn fmt(&self, formatador: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [o5, o4, o3, o2, o1, o0] = self.0;
        write!(
            formatador,
            "{o5:02X}:{o4:02X}:{o3:02X}:{o2:02X}:{o1:02X}:{o0:02X}"
        )
    }
}

/// O que pode estar errado num endereço escrito à mão.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ErroDeEndereco {
    /// Não são seis grupos separados por `:`.
    #[error("um endereço Bluetooth tem seis grupos separados por `:`")]
    Formato,
    /// Algum grupo não é um par hexadecimal.
    #[error("cada grupo de um endereço Bluetooth é um par hexadecimal, como `A0`")]
    Digito,
}

impl FromStr for BdAddr {
    type Err = ErroDeEndereco;

    /// Lê `AA:BB:CC:DD:EE:FF`, em maiúsculas ou minúsculas.
    ///
    /// Aceita também `-` como separador, porque é como o Windows mostra o endereço em alguns
    /// lugares e ninguém deveria precisar saber disso.
    fn from_str(texto: &str) -> core::result::Result<Self, Self::Err> {
        // A forma primeiro, os dígitos depois. Quem digitou `AC50DE47EB28` esqueceu os
        // separadores: responder "cada grupo é um par hexadecimal" mandaria essa pessoa conferir
        // os dígitos, que estão certos, em vez de pôr os dois-pontos que faltam.
        if texto.split([':', '-']).count() != OCTETOS {
            return Err(ErroDeEndereco::Formato);
        }
        let mut bytes = [0u8; OCTETOS];
        for (destino, grupo) in bytes.iter_mut().zip(texto.split([':', '-'])) {
            if grupo.len() != 2 {
                return Err(ErroDeEndereco::Digito);
            }
            *destino = u8::from_str_radix(grupo, 16).map_err(|_| ErroDeEndereco::Digito)?;
        }
        Ok(Self(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O notebook Fedora da bancada de teste.
    const BANCADA: BdAddr = BdAddr([0xAC, 0x50, 0xDE, 0x47, 0xEB, 0x28]);

    #[test]
    fn o_texto_sobrevive_a_ida_e_volta() {
        let texto = BANCADA.to_string();
        assert_eq!(texto, "AC:50:DE:47:EB:28");
        assert_eq!(texto.parse::<BdAddr>().expect("lê"), BANCADA);
    }

    #[test]
    fn minusculas_e_hifen_tambem_sao_aceitos() {
        // O Windows mostra o endereço com hífen em alguns lugares, e ninguém deveria precisar
        // saber disso para digitar.
        assert_eq!("ac:50:de:47:eb:28".parse::<BdAddr>().expect("lê"), BANCADA);
        assert_eq!("AC-50-DE-47-EB-28".parse::<BdAddr>().expect("lê"), BANCADA);
    }

    #[test]
    fn o_inteiro_do_winsock_sobrevive_a_ida_e_volta() {
        assert_eq!(BdAddr::de_u64(BANCADA.para_u64()), BANCADA);
        // O octeto que se lê primeiro é o mais significativo do inteiro.
        assert_eq!(BANCADA.para_u64(), 0x0000_AC50_DE47_EB28);
    }

    #[test]
    fn os_dezesseis_bits_de_cima_do_inteiro_sao_ignorados() {
        // Um `BTH_ADDR` carrega 48 bits; lixo nos de cima não pode virar outro endereço.
        assert_eq!(BdAddr::de_u64(0xFFFF_AC50_DE47_EB28), BANCADA);
    }

    #[test]
    fn um_endereco_sem_separadores_e_erro_de_formato_e_nao_de_digito() {
        // Os doze dígitos estão certos; o que falta são os dois-pontos. Apontar para os dígitos
        // mandaria a pessoa conferir justamente a parte que está boa.
        assert_eq!(
            "AC50DE47EB28".parse::<BdAddr>(),
            Err(ErroDeEndereco::Formato)
        );
    }

    #[test]
    fn grupos_a_menos_ou_a_mais_sao_erro_de_formato() {
        assert_eq!(
            "AC:50:DE:47:EB".parse::<BdAddr>(),
            Err(ErroDeEndereco::Formato)
        );
        assert_eq!(
            "AC:50:DE:47:EB:28:99".parse::<BdAddr>(),
            Err(ErroDeEndereco::Formato)
        );
    }

    #[test]
    fn com_a_forma_certa_o_erro_aponta_para_o_digito() {
        assert_eq!(
            "AC:50:DE:47:EB:ZZ".parse::<BdAddr>(),
            Err(ErroDeEndereco::Digito)
        );
        assert_eq!(
            "AC:50:DE:47:EB:123".parse::<BdAddr>(),
            Err(ErroDeEndereco::Digito)
        );
        assert_eq!(
            "A:50:DE:47:EB:28".parse::<BdAddr>(),
            Err(ErroDeEndereco::Digito)
        );
    }

    #[test]
    fn cada_erro_explica_o_que_esta_errado() {
        // As duas mensagens precisam ser diferentes e dizer o que fazer — é a única serventia de
        // haver duas variantes.
        let formato = ErroDeEndereco::Formato.to_string();
        let digito = ErroDeEndereco::Digito.to_string();
        assert_ne!(formato, digito);
        assert!(formato.contains("seis grupos"), "{formato}");
        assert!(digito.contains("hexadecimal"), "{digito}");
    }

    #[test]
    fn o_nulo_e_reconhecido() {
        assert!(BdAddr::NULO.e_nulo());
        assert!(!BANCADA.e_nulo());
        assert_eq!(BdAddr::NULO.to_string(), "00:00:00:00:00:00");
    }

    #[test]
    fn o_canal_esta_na_faixa_valida_do_rfcomm() {
        // Um canal RFCOMM vai de 1 a 30. Zero ou 31 não vinculam, e a falha só apareceria no
        // hardware.
        assert!((1..=30).contains(&CANAL), "canal {CANAL} fora da faixa");
    }
}
