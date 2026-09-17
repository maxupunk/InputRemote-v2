//! Tamanhos máximos do protocolo, em um lugar só.
//!
//! Um limite espalhado por vários arquivos é um limite que vai divergir. Todos os números
//! de tamanho do protocolo vivem aqui, com a origem de cada um documentada — a regra de
//! constante mágica de `docs/09-padroes-de-codigo.md` §10.

/// Máximo de texto claro num datagrama UDP.
///
/// Origem: `docs/03-protocolo.md` §2. Fica abaixo da MTU típica de 1500 B com folga para
/// cabeçalhos IP/UDP e para o contador e a etiqueta do Noise, de modo que um quadro nunca
/// seja fragmentado em IP.
pub const MAX_UDP_PLAINTEXT: usize = 1200;

/// Máximo de texto claro num quadro RFCOMM.
///
/// Origem: `docs/03-protocolo.md` §2. A MTU de RFCOMM varia entre pilhas Bluetooth; 512 B
/// é seguro em todas as observadas. O valor efetivo é negociado no enlace e pode ser
/// menor, nunca maior.
pub const MAX_RFCOMM_PLAINTEXT: usize = 512;

/// Máximo de texto claro num quadro TCP.
///
/// Origem: o teto do próprio Noise, **não** uma escolha nossa. Uma mensagem de transporte
/// Noise tem no máximo 65 535 B *contando a etiqueta Poly1305*, logo o texto claro para em
/// 65 519 B.
///
/// `docs/03-protocolo.md` §2 dizia 64 KiB, que é 65 536 — dezessete bytes acima do possível.
/// Não era margem apertada, era impossível: `snow` recusa `payload + 16 > 65535` com
/// `Error::Input`, então o primeiro bloco cheio de arquivo nunca teria sido cifrado. O número
/// passou dois meses sem doer porque nada usava TCP ainda. Ver
/// [ADR-0010](../../../docs/adr/0010-canal-de-dados-em-tcp-proprio.md).
///
/// Quem envia bloco de arquivo não usa este valor e sim [`MAX_FILE_BLOCK`], que desconta o
/// cabeçalho da mensagem.
pub const MAX_TCP_PLAINTEXT: usize = 65_519;

/// Máximo de bytes de conteúdo num `FileBlock`.
///
/// Origem: [`MAX_TCP_PLAINTEXT`] menos folga para o cabeçalho da mensagem — o byte do canal,
/// o discriminante, o identificador, o índice do item e o deslocamento. Um número redondo,
/// com folga deliberada, em vez do máximo aritmético: o ganho de encher os últimos bytes é
/// nulo e o custo de errar a conta é um enlace que cai no bloco cheio.
///
/// A folga é conferida por teste (`a_full_file_block_fits_a_tcp_frame`), não por confiança na
/// aritmética do `postcard`.
pub const MAX_FILE_BLOCK: usize = 60 * 1024;

/// Máximo que uma mensagem de **entrada** pode ocupar codificada.
///
/// Origem: `docs/03-protocolo.md` §9. É um teto deliberadamente apertado: se uma mensagem
/// de entrada passar disto, alguma coisa foi acrescentada ao caminho quente que não
/// deveria estar lá. O teste `input_messages_are_small` falha se for violado.
pub const MAX_INPUT_MESSAGE: usize = 64;

/// Máximo de texto de clipboard fora do portador TCP.
///
/// Origem: `docs/03-protocolo.md` §6, canal 4. Acima disto, o texto só viaja pelo canal 5.
pub const MAX_CLIPBOARD_TEXT_OFF_TCP: usize = 256 * 1024;

/// Máximo de monitores num `ScreenLayout`.
///
/// Origem: limite prático. Impede que um par remoto force alocação grande com um anúncio
/// de arranjo absurdo — o decodificador roda como `SYSTEM`.
pub const MAX_MONITORS: usize = 16;

/// Máximo de teclas pressionadas simultaneamente num `StateSnapshot`.
///
/// Origem: teclados USB comuns reportam no máximo 6 teclas sem N-key rollover; 32 cobre
/// N-key rollover com folga e mantém o snapshot pequeno.
pub const MAX_PRESSED_KEYS: usize = 32;

/// Máximo de bytes num nome legível de máquina.
///
/// Origem: limite prático, e o nome é exibido na interface.
pub const MAX_MACHINE_NAME: usize = 64;

/// Máximo de itens num manifesto de transferência de arquivos.
///
/// Origem: `docs/01-visao-e-escopo.md` §3.3. Cota aplicada antes de qualquer materialização.
pub const MAX_MANIFEST_ITEMS: usize = 10_000;

/// Máximo de bytes num caminho relativo dentro de um manifesto.
pub const MAX_RELATIVE_PATH: usize = 1024;

// As relações entre estes números são invariantes, não comportamento — então são conferidas
// em tempo de compilação, e não por teste. Um limite incoerente deixa de compilar, o que é
// estritamente melhor que falhar num teste que alguém pode marcar como ignorado.

/// O quadro UDP inteiro tem de caber num pacote IP sem fragmentar.
///
/// Texto claro + contador Noise (8) + etiqueta Poly1305 (16) + cabeçalho UDP (8) +
/// cabeçalho IPv6 (40), com folga para túneis comuns.
const _UDP_FITS_ONE_IP_PACKET: () = {
    const NOISE_OVERHEAD: usize = 8 + 16;
    const UDP_HEADER: usize = 8;
    const IPV6_HEADER: usize = 40;
    assert!(MAX_UDP_PLAINTEXT + NOISE_OVERHEAD + UDP_HEADER + IPV6_HEADER < 1500);
};

/// Uma mensagem de entrada tem de caber com folga em qualquer portador de entrada.
const _INPUT_FITS_EVERY_INPUT_CARRIER: () = {
    assert!(MAX_INPUT_MESSAGE < MAX_RFCOMM_PLAINTEXT);
    assert!(MAX_INPUT_MESSAGE < MAX_UDP_PLAINTEXT);
};

/// O canal de texto do clipboard só faz sentido se for maior que um quadro de entrada.
const _CLIPBOARD_IS_LARGER_THAN_INPUT: () = {
    assert!(MAX_CLIPBOARD_TEXT_OFF_TCP > MAX_INPUT_MESSAGE);
};

/// O quadro TCP tem de caber numa mensagem de transporte Noise.
///
/// O teto é do Noise, não nosso: 65 535 B por mensagem, etiqueta Poly1305 inclusa. Está
/// escrito como asserção porque foi precisamente esta conta que `docs/03` §2 errou — por
/// dezessete bytes, e sem doer, porque nada usava TCP.
const _TCP_FITS_ONE_NOISE_MESSAGE: () = {
    const MAX_NOISE_MESSAGE: usize = 65_535;
    const TAG: usize = 16;
    assert!(MAX_TCP_PLAINTEXT + TAG <= MAX_NOISE_MESSAGE);
};

/// Um bloco de arquivo tem de deixar espaço para o cabeçalho da mensagem que o carrega.
const _FILE_BLOCK_LEAVES_ROOM_FOR_ITS_HEADER: () = {
    assert!(MAX_FILE_BLOCK < MAX_TCP_PLAINTEXT);
};
