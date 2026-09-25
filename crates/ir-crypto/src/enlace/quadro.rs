//! O prefixo de tamanho dos portadores de *stream*.
//!
//! RFCOMM e TCP entregam bytes em ordem e sem perda, mas picados e colados como o meio quiser: não
//! existe "um pacote, uma mensagem". [03, §2](../../../docs/03-protocolo.md) manda um prefixo de
//! tamanho, em *little-endian*, antes de cada corpo — é ele que devolve a fronteira que o meio não
//! dá.
//!
//! Os dois portadores diferem só na largura do prefixo (`u16` no rádio, `u32` no TCP de arquivos) e
//! no teto do corpo; os dois entram aqui como parâmetros de tipo.

/// Um corpo maior que o teto do portador — anunciado pelo par ou pedido por nós.
///
/// Conferido **antes** de reservar memória: estes bytes chegam num processo privilegiado, vindos de
/// um rádio ou de uma rede que qualquer um alcança ([04, §1](../../../docs/04-seguranca.md)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Excesso {
    /// O tamanho anunciado ou pedido.
    pub tamanho: usize,
    /// O teto do portador.
    pub limite: usize,
}

/// Põe o prefixo de `PREFIXO` bytes num corpo, deixando-o pronto para o socket.
///
/// # Errors
///
/// [`Excesso`] se o corpo passa de `TETO`, ou se o tamanho dele não cabe no prefixo. Falhar aqui é
/// falha nossa, e custa menos que descobrir do outro lado.
pub fn enquadrar<const PREFIXO: usize, const TETO: usize>(
    corpo: &[u8],
) -> Result<Vec<u8>, Excesso> {
    let excesso = Excesso {
        tamanho: corpo.len(),
        limite: TETO,
    };
    let bytes = u64::try_from(corpo.len())
        .map_err(|_| excesso)?
        .to_le_bytes();
    let (prefixo, sobra) = bytes.split_at_checked(PREFIXO).ok_or(excesso)?;
    if corpo.len() > TETO || sobra.iter().any(|byte| *byte != 0) {
        return Err(excesso);
    }
    let mut saida = Vec::with_capacity(PREFIXO + corpo.len());
    saida.extend_from_slice(prefixo);
    saida.extend_from_slice(corpo);
    Ok(saida)
}

/// Junta os pedaços que chegam do *stream* e devolve um corpo completo de cada vez.
///
/// Alimente com o que o socket entregou, do tamanho que vier, e peça corpos até não haver mais
/// nenhum inteiro.
#[derive(Debug, Default)]
pub struct Desenquadrador<const PREFIXO: usize, const TETO: usize> {
    pendente: Vec<u8>,
}

impl<const PREFIXO: usize, const TETO: usize> Desenquadrador<PREFIXO, TETO> {
    /// Um desenquadrador vazio.
    #[must_use]
    pub const fn novo() -> Self {
        Self {
            pendente: Vec::new(),
        }
    }

    /// Guarda os bytes que acabaram de chegar do socket.
    pub fn alimentar(&mut self, bytes: &[u8]) {
        self.pendente.extend_from_slice(bytes);
    }

    /// Quantos bytes ainda não formaram um corpo inteiro.
    #[must_use]
    pub fn pendentes(&self) -> usize {
        self.pendente.len()
    }

    /// O próximo corpo completo, se já chegou inteiro.
    ///
    /// # Errors
    ///
    /// [`Excesso`] se o tamanho anunciado passa de `TETO` — conferido antes de reservar memória.
    /// Um anúncio absurdo derruba o enlace, e não a máquina.
    pub fn proximo(&mut self) -> Result<Option<Vec<u8>>, Excesso> {
        let Some(prefixo) = self.pendente.get(..PREFIXO) else {
            return Ok(None); // nem o tamanho chegou ainda
        };
        let anunciado = prefixo
            .iter()
            .rev()
            .fold(0u64, |soma, byte| (soma << 8) | u64::from(*byte));
        let tamanho = usize::try_from(anunciado).unwrap_or(usize::MAX);
        if tamanho > TETO {
            return Err(Excesso {
                tamanho,
                limite: TETO,
            });
        }
        let fim = PREFIXO + tamanho;
        if self.pendente.len() < fim {
            return Ok(None); // o corpo ainda está chegando
        }
        Ok(Some(self.pendente.drain(..fim).skip(PREFIXO).collect()))
    }
}
