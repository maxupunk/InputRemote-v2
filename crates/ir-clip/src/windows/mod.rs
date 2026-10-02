//! O backend do Windows.
//!
//! Três peças: o acesso ao clipboard ([`area`], onde vive o `unsafe`), o aviso de mudança
//! ([`vigia`], que é uma janela sem tela) e a promessa de arquivos que ainda estão chegando
//! ([`promessa`], arquivos virtuais que o Explorer cola antes de eles chegarem).
//!
//! # A repetição, e por que ela é curta
//!
//! `OpenClipboard` falha enquanto outro processo tem o clipboard aberto, e isso acontece: o Office
//! o abre ao copiar, o Explorer também. A resposta é tentar de novo por pouco tempo.
//!
//! Pouco de propósito. Se em meio segundo o clipboard não abriu, quem o segura está travado — e
//! insistir mais só transformaria o problema dele em lentidão nossa. Perde-se aquela mudança e se
//! espera a próxima, que é o comportamento que o usuário percebe como "não aconteceu nada" em vez
//! de "o programa congelou".

mod area;
mod promessa;
mod vigia;

use std::thread::sleep;
use std::time::Duration;

use crate::conteudo::Conteudo;
use crate::error::{ClipError, Result};
use crate::{Clipboard, Vigia};

/// Quantas vezes tentar abrir o clipboard ocupado.
const TENTATIVAS: u32 = 10;

/// Quanto esperar entre tentativas. Dez vezes 50 ms é meio segundo no total.
const ESPERA: Duration = Duration::from_millis(50);

/// O clipboard desta sessão do Windows.
#[derive(Debug, Default)]
pub struct ClipboardDoWindows {
    /// A promessa de arquivos que estão chegando, criada na primeira.
    promessa: Option<promessa::Promessa>,
}

impl Clipboard for ClipboardDoWindows {
    fn ler(&mut self) -> Result<Option<Conteudo>> {
        // A promessa é nossa: não é conteúdo novo do usuário, e não volta para o par.
        if promessa::e_dona() {
            return Ok(None);
        }
        repetir(area::ler)
    }

    fn publicar(&mut self, conteudo: &Conteudo) -> Result<()> {
        if let Some(promessa) = &mut self.promessa {
            match conteudo {
                Conteudo::Arquivos(_) => {
                    if promessa.cumprir() == promessa::Cumprimento::CopiaMaisNova {
                        // A pessoa copiou outra coisa enquanto os arquivos chegavam: a cópia dela
                        // vale. Quem colou durante a espera continua lendo de onde eles ficaram.
                        return Ok(());
                    }
                }
                _ => promessa.desfazer(),
            }
        }
        repetir(|| area::publicar(conteudo))
    }

    fn prometer_arquivos(&mut self, chegada: &crate::Chegada) -> Result<()> {
        if self.promessa.is_none() {
            self.promessa = Some(promessa::Promessa::nova()?);
        }
        match &mut self.promessa {
            Some(promessa) => promessa.prometer(chegada),
            None => Ok(()),
        }
    }

    fn desfazer_promessa(&mut self) {
        if let Some(promessa) = &mut self.promessa {
            promessa.desfazer();
        }
    }
}

/// Tenta, e repete só enquanto o motivo for "ocupado".
///
/// Genérico sobre a operação para a política de repetição existir **num** lugar: duas cópias dela
/// divergiriam, e a de publicar é a que não pode falhar em silêncio.
fn repetir<T>(mut operacao: impl FnMut() -> Result<T>) -> Result<T> {
    let mut ultimo = ClipError::Ocupado;
    for _ in 0..TENTATIVAS {
        match operacao() {
            Ok(valor) => return Ok(valor),
            Err(erro) if erro.vale_repetir() => {
                ultimo = erro;
                sleep(ESPERA);
            }
            Err(erro) => return Err(erro),
        }
    }
    Err(ultimo)
}

/// Abre o clipboard desta sessão.
///
/// # Errors
///
/// Não falha hoje: o acesso é por chamada, e a indisponibilidade aparece na primeira leitura — que é
/// onde ela é verdade. No desktop `Winlogon` não há clipboard de usuário, e é a leitura que descobre.
pub fn abrir() -> Result<Box<dyn Clipboard>> {
    Ok(Box::new(ClipboardDoWindows::default()))
}

/// Começa a vigiar as mudanças.
///
/// # Errors
///
/// [`ClipError::Indisponivel`] se a janela de aviso não puder ser criada — o que acontece onde não
/// há *window station* alcançável.
pub fn vigiar() -> Result<Box<dyn Vigia>> {
    Ok(Box::new(vigia::VigiaDoWindows::nova()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repetir_desiste_depois_do_teto_em_vez_de_travar() {
        // O ponto: se quem segura o clipboard está travado, insistir para sempre transformaria o
        // problema dele em congelamento nosso.
        let mut tentativas = 0;
        let resultado: Result<()> = repetir(|| {
            tentativas += 1;
            Err(ClipError::Ocupado)
        });
        assert!(resultado.is_err());
        assert_eq!(tentativas, TENTATIVAS);
    }

    #[test]
    fn repetir_para_no_primeiro_sucesso() {
        let mut tentativas = 0;
        let resultado = repetir(|| {
            tentativas += 1;
            if tentativas < 3 {
                Err(ClipError::Ocupado)
            } else {
                Ok(42)
            }
        });
        assert_eq!(resultado.ok(), Some(42));
        assert_eq!(tentativas, 3);
    }

    #[test]
    fn um_erro_que_nao_melhora_nao_e_repetido() {
        // Repetir um formato não suportado seria um laço quente por cima de uma condição estável.
        let mut tentativas = 0;
        let resultado: Result<()> = repetir(|| {
            tentativas += 1;
            Err(ClipError::FormatoNaoSuportado)
        });
        assert!(resultado.is_err());
        assert_eq!(tentativas, 1);
    }

    #[test]
    fn a_espera_total_e_curta_o_bastante_para_o_usuario_nao_notar() {
        // Meio segundo. Acima disso, "copiei e nada aconteceu" viraria "o programa travou".
        let total = ESPERA * TENTATIVAS;
        assert!(total <= Duration::from_millis(600), "{total:?}");
    }

    /// Mexe no clipboard de verdade de quem roda: só à mão, `--ignored`.
    #[test]
    #[ignore = "troca o clipboard do usuário"]
    fn uma_imagem_publicada_volta_igual_do_clipboard_de_verdade() {
        let dib = {
            // Um PNG de 2×1 feito pelo próprio conversor, a partir de um DIB conhecido.
            let mut dib = Vec::new();
            dib.extend_from_slice(&40u32.to_le_bytes());
            dib.extend_from_slice(&2i32.to_le_bytes());
            dib.extend_from_slice(&1i32.to_le_bytes());
            dib.extend_from_slice(&1u16.to_le_bytes());
            dib.extend_from_slice(&32u16.to_le_bytes());
            dib.extend_from_slice(&[0; 24]);
            dib.extend_from_slice(&[0, 0, 255, 255, 255, 0, 0, 255]);
            dib
        };
        let png = crate::imagem::png_de_dib(&dib).unwrap();
        let mut clip = abrir().unwrap();
        clip.publicar(&Conteudo::Imagem(png.clone())).unwrap();
        assert_eq!(clip.ler().unwrap(), Some(Conteudo::Imagem(png)));
    }

    /// Lê o que outro programa pôs — rode depois de copiar uma imagem, `--ignored`.
    #[test]
    #[ignore = "precisa de uma imagem copiada por outro programa"]
    fn uma_imagem_de_outro_programa_e_lida_como_png() {
        let Some(Conteudo::Imagem(png)) = abrir().unwrap().ler().unwrap() else {
            panic!("não havia imagem no clipboard");
        };
        assert!(png.starts_with(&[0x89, b'P', b'N', b'G']), "não saiu PNG");
        assert!(crate::imagem::dib_de_png(&png).is_ok());
    }

    /// Uma cópia chegando, prometida no clipboard, para colar à mão no Explorer: `--ignored`.
    ///
    /// Rode, abra uma pasta no Explorer e aperte Ctrl+V nos primeiros segundos: o diálogo de cópia
    /// do Explorer aparece na hora, e `grande.bin` termina de colar logo depois do último megabyte
    /// chegar — inteiro, lido da montagem enquanto chega e de onde a entrega foi publicada depois.
    /// Assim foi provado na bancada (log 56); colar pelo `InvokeVerb` de outro processo não serve
    /// de prova, porque a extração de arquivos virtuais precisa do processo do Explorer.
    #[test]
    #[ignore = "troca o clipboard do usuário; colar à mão no Explorer"]
    fn uma_copia_chegando_para_colar_no_explorer() {
        use std::io::Write as _;

        let base = std::env::temp_dir().join("ir-promessa-para-colar");
        let _ = std::fs::remove_dir_all(&base);
        let (montagem, publicada) = (base.join("montagem"), base.join("publicada"));
        std::fs::create_dir_all(&montagem).unwrap();
        let pedaco = vec![b'x'; 1_000_000];
        let chegada = crate::Chegada {
            montagem: montagem.clone(),
            publicada_em: publicada.clone(),
            itens: vec![crate::ItemDaChegada {
                caminho: "grande.bin".to_owned(),
                tamanho: 3 * 1_000_000,
                pasta: false,
            }],
        };
        let mut clip = ClipboardDoWindows::default();
        clip.prometer_arquivos(&chegada).unwrap();
        let mut arquivo = std::fs::File::create(montagem.join("grande.bin")).unwrap();
        for _ in 0..3 {
            std::thread::sleep(Duration::from_secs(2));
            arquivo.write_all(&pedaco).unwrap();
            arquivo.flush().unwrap();
        }
        drop(arquivo);
        std::fs::rename(&montagem, &publicada).unwrap();
        clip.publicar(&Conteudo::Arquivos(vec![publicada.join("grande.bin")]))
            .unwrap();
        std::thread::sleep(Duration::from_secs(20));
    }
}
