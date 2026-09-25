//! Conversão entre pixels e frações normalizadas.
//!
//! A fração `0..=u16::MAX` é o que atravessa a rede numa travessia de borda: ela é
//! independente de resolução, então o meio da borda de um monitor 4K é o meio da borda de um
//! 720p do outro lado.

/// Converte um deslocamento em pixels dentro de `span` numa fração `0..=u16::MAX`.
///
/// Arredonda para o mais próximo, e não trunca. A diferença não é cosmética: numa tela de
/// 1920 px, um pixel vale 34 unidades de fração, e truncar nas duas conversões perdia o
/// recuo de um pixel do ponto de entrada — o ponteiro voltava exatamente para a borda e
/// atravessava de novo. Foi o teste `entry_is_never_on_the_far_edge` que pegou isso.
pub(super) fn normalise(offset: i64, span: u32) -> u16 {
    let last = u64::from(span).saturating_sub(1);
    if last == 0 {
        return 0;
    }
    let clamped = u64::try_from(offset.max(0)).unwrap_or(0).min(last);
    // Em u64: `last` cabe em u32 e u16::MAX é pequeno, então o produto não estoura.
    let scaled = (clamped * u64::from(u16::MAX) + last / 2) / last;
    u16::try_from(scaled).unwrap_or(u16::MAX)
}

/// Converte uma fração `0..=u16::MAX` num deslocamento em pixels dentro de `span`.
///
/// Arredonda para o mais próximo, pelo mesmo motivo de [`normalise`].
pub(super) fn denormalise(fraction: u16, span: u32) -> i32 {
    let last = u64::from(span).saturating_sub(1);
    let full = u64::from(u16::MAX);
    let offset = (u64::from(fraction) * last + full / 2) / full;
    i32::try_from(offset).unwrap_or(i32::MAX)
}

/// Converte uma fração de um trecho `inner`, que começa `offset` pixels depois do início de um
/// trecho `outer`, na fração correspondente de `outer`.
///
/// É a posição dentro de um monitor vista no desktop virtual inteiro. Uma divisão só, arredondada
/// como [`normalise`]: passar por pixels arredondaria duas vezes e deslocaria até meio pixel. Com os
/// dois trechos iguais e sem deslocamento — uma tela só —, devolve a própria fração.
pub(crate) fn reframe(fraction: u16, offset: i64, inner: u32, outer: u32) -> u16 {
    let outer_last = u64::from(outer).saturating_sub(1);
    if outer_last == 0 {
        return 0;
    }
    let full = u64::from(u16::MAX);
    let inner_last = u64::from(inner).saturating_sub(1);
    let offset = u64::try_from(offset.max(0)).unwrap_or(0);
    // O pixel exato é `offset + fraction * inner_last / full`; a fração dele em `outer` é esse
    // pixel vezes `full / outer_last`. Em u64: cada parcela cabe em 48 bits.
    let numerator = offset * full + u64::from(fraction) * inner_last;
    let scaled = (numerator + outer_last / 2) / outer_last;
    u16::try_from(scaled.min(full)).unwrap_or(u16::MAX)
}

/// O deslocamento da última coluna ou linha de um retângulo de largura `span`.
///
/// Saturado: uma dimensão maior que `i32::MAX` não existe em tela real, e saturar é melhor
/// que estourar num tipo que veio da rede.
pub(super) fn span_as_i32(span: u32) -> i32 {
    i32::try_from(span.saturating_sub(1)).unwrap_or(i32::MAX)
}

/// A dimensão de um retângulo cujas bordas inclusivas são `low` e `high`.
pub(super) fn span_from_bounds(low: i32, high: i32) -> u32 {
    let span = i64::from(high) - i64::from(low) + 1;
    u32::try_from(span.max(1)).unwrap_or(u32::MAX)
}

/// `clamp` de `i32` utilizável em contexto constante.
pub(super) const fn clamp_i32(value: i32, low: i32, high: i32) -> i32 {
    if value < low {
        low
    } else if value > high {
        high
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalisation_round_trips_exactly_for_every_common_resolution() {
        // Regressão do defeito que o teste `entry_is_never_on_the_far_edge` pegou: com
        // truncamento, um pixel de recuo virava zero na volta e o ponteiro quicava na borda.
        // A ida e volta é exata enquanto a tela couber em 65 536 px, o que cobre qualquer
        // resolução real com folga.
        for span in [2u32, 3, 100, 800, 1280, 1366, 1920, 2560, 3840, 7680] {
            let last = i64::from(span) - 1;
            for px in [0, 1, 2, last / 2, last - 1, last] {
                if !(0..=last).contains(&px) {
                    continue; // spans pequenos não têm todos esses pixels
                }
                let fraction = normalise(px, span);
                let back = i64::from(denormalise(fraction, span));
                assert_eq!(back, px, "span={span}, px={px}, fração={fraction}");
            }
        }
    }

    #[test]
    fn reframing_into_the_same_span_is_the_identity() {
        for span in [1u32, 2, 800, 1920, 3840] {
            for fraction in [0u16, 1, 17, 32_767, 65_534, u16::MAX] {
                let expected = if span == 1 { 0 } else { fraction };
                assert_eq!(reframe(fraction, 0, span, span), expected, "span={span}");
            }
        }
    }

    #[test]
    fn reframing_agrees_with_the_pixel_it_names() {
        // O pixel 1920 de um trecho de 3200, visto de dentro de um trecho de 1280 que começa nele.
        let fraction = normalise(0, 1280);
        assert_eq!(reframe(fraction, 1920, 1280, 3200), normalise(1920, 3200));
        let last = normalise(1279, 1280);
        assert_eq!(reframe(last, 1920, 1280, 3200), u16::MAX);
    }

    #[test]
    fn a_one_pixel_span_is_always_the_only_pixel() {
        assert_eq!(normalise(0, 1), 0);
        assert_eq!(normalise(999, 1), 0);
        assert_eq!(denormalise(u16::MAX, 1), 0);
    }
}
