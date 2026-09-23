//! Vetores gravados do formato de fio, versão 3.
//!
//! Este arquivo é a única proteção contra a falha mais perigosa deste protocolo.
//!
//! O `postcard` não é autodescritivo: os campos são posicionais. Reordenar dois campos de
//! uma struct, inserir uma variante no meio de um enum ou trocar `u16` por `u32` produz
//! bytes que a outra ponta **decodifica com sucesso** e interpreta errado. Num produto que
//! digita senha em tela de bloqueio, isso não é um bug de compatibilidade — é digitar a
//! coisa errada na máquina do outro.
//!
//! Nenhum teste de ida e volta pega isso, porque as duas pontas do teste mudam juntas.
//! Só um byte gravado pega.
//!
//! # Quando este teste falhar
//!
//! Ele falha por um de dois motivos, e a resposta é diferente:
//!
//! 1. **A mudança foi intencional.** Incremente `version::CURRENT`, acrescente o conjunto
//!    novo de vetores mantendo o antigo, e trate a versão antiga na negociação.
//! 2. **A mudança foi acidental.** Reverta. Foi exatamente para isto que o teste existe.
//!
//! Apagar ou reescrever um vetor para "fazer o teste passar" desfaz a única proteção que
//! existe aqui. Ver `docs/03-protocolo.md` §9.
//!
//! # A exceção de pré-lançamento
//!
//! Enquanto a versão 1 não tiver sido lançada — enquanto não houver um par instalado em lugar
//! nenhum falando este protocolo —, uma mudança intencional atualiza o vetor **no lugar**, em
//! vez de acrescentar um conjunto novo: não há com quem manter compatibilidade. Toda
//! atualização dessas fica registrada em `docs/logs/`, com o motivo.
//!
//! A exceção acaba no primeiro lançamento. Depois dele, a regra acima vale sem ressalva.

// Um teste de integração é um crate próprio, então a liberação de `expect` que a biblioteca
// concede a `#[cfg(test)]` não chega até aqui. Em teste, `expect` com mensagem é melhor que
// propagar erro: a mensagem *é* o diagnóstico.
#![allow(clippy::expect_used)]

mod dados;
mod table;

use ir_proto::carrier::Carrier;
use ir_proto::channel::ChannelId;
use ir_proto::codec;
use table::{Vector, vectors};

/// Com qual portador cada vetor é codificado.
///
/// Quase todos usam UDP — o portador mais apertado, e portanto o que mais denuncia quadro
/// grande. O canal 5 é a exceção **obrigatória**: `ChannelId::Bulk.allows(Udp)` é falso por
/// desenho, e pedir ao codec que o codifique em UDP seria pedir que ele quebrasse a regra que
/// existe para impedir arquivo de disputar o portador da entrada.
fn carrier_for(channel: ChannelId) -> Carrier {
    if channel == ChannelId::Bulk {
        Carrier::Tcp
    } else {
        Carrier::Udp
    }
}

fn to_hex(bytes: &[u8]) -> String {
    use core::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(out, "{byte:02x}").expect("escrever em String não falha");
    }
    out
}

fn from_hex(hex: &str) -> Vec<u8> {
    assert!(
        hex.len().is_multiple_of(2),
        "hex com número ímpar de dígitos"
    );
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            let pair = hex.get(i..i + 2).expect("par de dígitos");
            u8::from_str_radix(pair, 16).expect("dígito hexadecimal válido")
        })
        .collect()
}

#[test]
fn encoding_matches_the_recorded_bytes() {
    for Vector { name, frame, hex } in vectors() {
        let bytes = codec::encode(&frame, carrier_for(frame.channel())).expect("codifica");
        assert_eq!(
            to_hex(&bytes),
            hex,
            "\no formato de fio de `{name}` mudou.\n\
             Se a mudança foi intencional, incremente version::CURRENT e adicione um \
             conjunto novo de vetores.\n\
             Se não foi, reverta — este teste existe exatamente para pegar isto.\n"
        );
    }
}

#[test]
fn recorded_bytes_decode_back_to_the_same_frame() {
    for Vector { name, frame, hex } in vectors() {
        let decoded =
            codec::decode(&from_hex(hex), carrier_for(frame.channel())).expect("decodifica");
        assert_eq!(decoded, frame, "`{name}` não sobreviveu à ida e volta");
    }
}

#[test]
fn the_first_byte_is_always_the_channel() {
    // A regra de docs/03-protocolo.md §4. Verificada contra os bytes gravados, e não contra
    // o que o código calcula, para que ela não possa "mudar junto".
    for Vector { name, frame, hex } in vectors() {
        let first = from_hex(hex).first().copied().expect("vetor não vazio");
        assert_eq!(
            first,
            frame.channel().to_wire(),
            "primeiro byte de `{name}`"
        );
    }
}

#[test]
fn every_recorded_vector_has_a_distinct_name_and_encoding() {
    let all = vectors();
    for (index, a) in all.iter().enumerate() {
        for b in all.iter().skip(index + 1) {
            assert_ne!(a.name, b.name, "nome de vetor repetido");
            assert_ne!(a.hex, b.hex, "`{}` e `{}` codificam igual", a.name, b.name);
        }
    }
}

#[test]
fn input_vectors_stay_within_the_hot_path_budget() {
    for Vector { name, frame, hex } in vectors() {
        if frame.message.is_hot_path() {
            let size = from_hex(hex).len();
            assert!(
                size <= ir_proto::limits::MAX_INPUT_MESSAGE,
                "`{name}` ocupa {size} B, teto {} B",
                ir_proto::limits::MAX_INPUT_MESSAGE
            );
        }
    }
}

#[test]
fn the_vector_set_covers_every_channel() {
    // Eram quatro canais. Clipboard e dados ficaram de fora enquanto não tinham conteúdo, e
    // a lista escrita à mão não tinha como avisar que faltavam. Agora a fonte é
    // `ChannelId::ALL`: um canal novo entra aqui sozinho, e o teste cobra o vetor.
    let covered: Vec<ChannelId> = vectors().iter().map(|v| v.frame.channel()).collect();
    for channel in ChannelId::ALL {
        assert!(
            covered.contains(&channel),
            "nenhum vetor cobre o canal {channel}"
        );
    }
}

#[test]
fn every_message_variant_of_the_data_channels_is_recorded() {
    // `docs/03-protocolo.md` §9 exige ida e volta de toda variante, e os canais 4 e 5 são os
    // que carregam conteúdo de tamanho variável — onde um erro de posição não corrompe uma
    // tecla, corrompe um arquivo. A conta é escrita aqui para que acrescentar variante ao
    // protocolo sem gravar o vetor correspondente falhe.
    const VARIANTES_DE_CLIPBOARD: usize = 5;
    const VARIANTES_DE_DADOS: usize = 9;

    let conta = |canal: ChannelId| {
        vectors()
            .iter()
            .filter(|v| v.frame.channel() == canal)
            .count()
    };
    assert_eq!(conta(ChannelId::ClipboardText), VARIANTES_DE_CLIPBOARD);
    assert_eq!(conta(ChannelId::Bulk), VARIANTES_DE_DADOS);
}
