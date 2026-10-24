//! As funções que o Kotlin chama (`io.github.inputremote.painel.nucleo.Nativo`).
//!
//! Só tradução de tipos: texto do Java para `String`, o resultado de volta como texto JSON
//! ([`crate::ponte`]) ou como o número de uma ligação. Nenhum ponteiro cru atravessa — o Kotlin
//! guarda um número, e o registro daqui sabe a que ligação ele se refere. Um número velho ou
//! inventado não acha nada, e não derruba nada.
//!
//! Cada função sobe o ambiente com `with_env`, que também segura um pânico antes da fronteira: um
//! pânico que atravessasse para a JVM derrubaria o aplicativo inteiro.

// `#[unsafe(no_mangle)]` é a única forma de o JNI achar a função pelo nome, e só ele é `unsafe`
// aqui: o corpo de cada função é Rust seguro. Exceção declarada em 09, §4.
#![allow(unsafe_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jint, jlong};
use jni::{Env, EnvUnowned};

use ir_monitor::Interesse;

use crate::Ligacao;
use crate::cliente::{Comando, Destino};
use crate::cofre::Cofre;
use crate::{descoberta, ponte};

type Registro = Mutex<HashMap<jlong, Arc<Ligacao>>>;

fn registro() -> &'static Registro {
    static LIGACOES: OnceLock<Registro> = OnceLock::new();
    LIGACOES.get_or_init(Registro::default)
}

fn guardar(ligacao: Ligacao) -> jlong {
    static PROXIMO: AtomicI64 = AtomicI64::new(1);
    let id = PROXIMO.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut ligacoes) = registro().lock() {
        ligacoes.insert(id, Arc::new(ligacao));
    }
    id
}

fn achar(id: jlong) -> Option<Arc<Ligacao>> {
    registro().lock().ok()?.get(&id).cloned()
}

fn texto(env: &Env<'_>, valor: &JString<'_>) -> jni::errors::Result<String> {
    valor.try_to_string(env)
}

fn milissegundos(valor: jint) -> Duration {
    Duration::from_millis(u64::try_from(valor).unwrap_or(0))
}

/// Procura computadores na rede por `duracao_ms` e devolve a lista em JSON.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_inputremote_painel_nucleo_Nativo_procurar<'c>(
    mut env: EnvUnowned<'c>,
    _classe: JClass<'c>,
    duracao_ms: jint,
) -> JString<'c> {
    env.with_env(|env| {
        let achados = descoberta::procurar(milissegundos(duracao_ms));
        JString::from_str(env, ponte::achados(&achados))
    })
    .resolve::<ThrowRuntimeExAndDefault>()
}

/// Os computadores pareados, em JSON.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_inputremote_painel_nucleo_Nativo_pareados<'c>(
    mut env: EnvUnowned<'c>,
    _classe: JClass<'c>,
    pasta: JString<'c>,
) -> JString<'c> {
    env.with_env(|env| {
        let pasta = PathBuf::from(texto(env, &pasta)?);
        let lista = Cofre::abrir(&pasta).map_or_else(
            |_| "[]".to_owned(),
            |cofre| ponte::pareados(cofre.computadores()),
        );
        JString::from_str(env, lista)
    })
    .resolve::<ThrowRuntimeExAndDefault>()
}

/// Esquece o computador desta chave.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_inputremote_painel_nucleo_Nativo_esquecer<'c>(
    mut env: EnvUnowned<'c>,
    _classe: JClass<'c>,
    pasta: JString<'c>,
    chave: JString<'c>,
) {
    env.with_env(|env| -> jni::errors::Result<()> {
        let pasta = PathBuf::from(texto(env, &pasta)?);
        let chave = texto(env, &chave)?;
        if let Ok(mut cofre) = Cofre::abrir(&pasta) {
            let _ = cofre.esquecer(&chave);
        }
        Ok(())
    })
    .resolve::<ThrowRuntimeExAndDefault>();
}

/// Começa a parear com o computador deste IP. Devolve o número da ligação, ou 0.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_inputremote_painel_nucleo_Nativo_parear<'c>(
    mut env: EnvUnowned<'c>,
    _classe: JClass<'c>,
    pasta: JString<'c>,
    aparelho: JString<'c>,
    computador: JString<'c>,
    endereco: JString<'c>,
) -> jlong {
    env.with_env(|env| -> jni::errors::Result<jlong> {
        let pasta = PathBuf::from(texto(env, &pasta)?);
        let aparelho = texto(env, &aparelho)?;
        let nome = texto(env, &computador)?;
        let Ok(ip) = texto(env, &endereco)?.parse() else {
            return Ok(0);
        };
        let endereco = descoberta::do_painel(ip);
        let destino = Destino::Parear { nome, endereco };
        Ok(Ligacao::abrir(pasta, aparelho, destino).map_or(0, guardar))
    })
    .resolve::<ThrowRuntimeExAndDefault>()
}

/// Volta ao computador pareado mais recente. Devolve o número da ligação, ou 0 sem pareado.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_inputremote_painel_nucleo_Nativo_voltar<'c>(
    mut env: EnvUnowned<'c>,
    _classe: JClass<'c>,
    pasta: JString<'c>,
    aparelho: JString<'c>,
) -> jlong {
    env.with_env(|env| -> jni::errors::Result<jlong> {
        let pasta = PathBuf::from(texto(env, &pasta)?);
        let aparelho = texto(env, &aparelho)?;
        let computador = Cofre::abrir(&pasta)
            .ok()
            .and_then(|cofre| cofre.computadores().first().cloned());
        let Some(computador) = computador else {
            return Ok(0);
        };
        Ok(Ligacao::abrir(pasta, aparelho, Destino::Voltar(computador)).map_or(0, guardar))
    })
    .resolve::<ThrowRuntimeExAndDefault>()
}

/// O próximo evento da ligação em JSON, esperando até `espera_ms`; texto vazio sem evento.
///
/// Depois do evento `caiu`, a ligação sai do registro: o número não serve mais.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_inputremote_painel_nucleo_Nativo_proximo<'c>(
    mut env: EnvUnowned<'c>,
    _classe: JClass<'c>,
    id: jlong,
    espera_ms: jint,
) -> JString<'c> {
    env.with_env(|env| {
        let evento = achar(id).and_then(|ligacao| ligacao.proximo(milissegundos(espera_ms)));
        if let Some(crate::cliente::Evento::Caiu { .. }) = &evento {
            soltar(id);
        }
        JString::from_str(env, evento.as_ref().map(ponte::evento).unwrap_or_default())
    })
    .resolve::<ThrowRuntimeExAndDefault>()
}

/// A resposta desta tela ao código.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_inputremote_painel_nucleo_Nativo_confirmar<'c>(
    _env: EnvUnowned<'c>,
    _classe: JClass<'c>,
    id: jlong,
    conferiu: jboolean,
) {
    comandar(id, Comando::Confirmar(conferiu));
}

/// Começa (`true`) ou para de receber o quadro.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_inputremote_painel_nucleo_Nativo_assinar<'c>(
    _env: EnvUnowned<'c>,
    _classe: JClass<'c>,
    id: jlong,
    quer: jboolean,
) {
    comandar(id, Comando::Assinar(quer));
}

/// O que a tela quer além do quadro: os nomes dos processos dos cards, em JSON (`["docker"]`), e
/// de qual computador a lista de processos está aberta — a posição dele no quadro, ou -1.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_inputremote_painel_nucleo_Nativo_interesse<'c>(
    mut env: EnvUnowned<'c>,
    _classe: JClass<'c>,
    id: jlong,
    processos: JString<'c>,
    lista: jint,
) {
    env.with_env(|env| -> jni::errors::Result<()> {
        let processos = ponte::nomes(&texto(env, &processos)?);
        let lista = u8::try_from(lista).ok();
        comandar(id, Comando::Interesse(Interesse { processos, lista }));
        Ok(())
    })
    .resolve::<ThrowRuntimeExAndDefault>();
}

/// Encerra a ligação e a tira do registro.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_inputremote_painel_nucleo_Nativo_fechar<'c>(
    _env: EnvUnowned<'c>,
    _classe: JClass<'c>,
    id: jlong,
) {
    comandar(id, Comando::Fechar);
    soltar(id);
}

fn comandar(id: jlong, comando: Comando) {
    if let Some(ligacao) = achar(id) {
        ligacao.comandar(comando);
    }
}

fn soltar(id: jlong) {
    if let Ok(mut ligacoes) = registro().lock() {
        ligacoes.remove(&id);
    }
}
