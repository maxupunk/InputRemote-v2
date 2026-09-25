//! Os monitores desta sessão.
//!
//! O agente contava só a tela principal, uma vez na subida: com dois monitores, o segundo não
//! existia para o par — o ponteiro não chegava nele, e a borda de travessia podia cair no meio do
//! desktop. Agora o arranjo inteiro vai, e o agente o conta de novo quando muda.

#![allow(unsafe_code)]

use ir_proto::ids::MonitorId;
use ir_proto::screens::{MonitorInfo, ScreenLayout};
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};
use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;

/// Um monitor como o Windows o descreve: o retângulo e se é o principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Retangulo {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) largura: u32,
    pub(crate) altura: u32,
    pub(crate) principal: bool,
}

/// O arranjo de telas desta sessão, ou nada se o Windows não disser.
#[must_use]
pub fn arranjo() -> Option<ScreenLayout> {
    let mut retangulos: Vec<Retangulo> = Vec::new();
    // SAFETY: o callback só escreve no `Vec` passado por `LPARAM`, que vive até o fim da chamada.
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(cada_monitor),
            LPARAM(std::ptr::from_mut(&mut retangulos) as isize),
        );
    }
    montar(&retangulos)
}

/// Monta o arranjo dos retângulos, com o principal primeiro. Separado para ser testado.
pub(crate) fn montar(retangulos: &[Retangulo]) -> Option<ScreenLayout> {
    let mut ordenados = retangulos.to_vec();
    ordenados.sort_by_key(|r| (!r.principal, r.x, r.y));
    let monitores: Vec<MonitorInfo> = ordenados
        .iter()
        .enumerate()
        .filter_map(|(indice, r)| {
            Some(MonitorInfo {
                id: MonitorId(u8::try_from(indice).ok()?),
                x: r.x,
                y: r.y,
                width: r.largura,
                height: r.altura,
                scale_permille: 1000,
                primary: r.principal,
            })
        })
        .collect();
    if monitores.is_empty() {
        return None;
    }
    ScreenLayout::new(monitores).ok()
}

/// O callback de `EnumDisplayMonitors`: acrescenta o monitor ao `Vec` apontado por `dados`.
unsafe extern "system" fn cada_monitor(
    monitor: HMONITOR,
    _: HDC,
    _: *mut RECT,
    dados: LPARAM,
) -> windows::core::BOOL {
    let mut info = MONITORINFO {
        cbSize: u32::try_from(size_of::<MONITORINFO>()).unwrap_or(0),
        ..Default::default()
    };
    // SAFETY: `info` tem o tamanho declarado; `monitor` veio do próprio Windows.
    if unsafe { GetMonitorInfoW(monitor, std::ptr::from_mut(&mut info)) }.as_bool() {
        let r = info.rcMonitor;
        if let (Ok(largura), Ok(altura)) = (
            u32::try_from(r.right - r.left),
            u32::try_from(r.bottom - r.top),
        ) {
            // SAFETY: `dados` é o `&mut Vec` que `arranjo` passou, vivo durante a enumeração.
            let lista = unsafe { &mut *(dados.0 as *mut Vec<Retangulo>) };
            lista.push(Retangulo {
                x: r.left,
                y: r.top,
                largura,
                altura,
                principal: info.dwFlags & MONITORINFOF_PRIMARY != 0,
            });
        }
    }
    true.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dois_monitores_viram_um_arranjo_com_o_principal_primeiro() {
        let arranjo = montar(&[
            Retangulo {
                x: 1920,
                y: 0,
                largura: 1280,
                altura: 1024,
                principal: false,
            },
            Retangulo {
                x: 0,
                y: 0,
                largura: 1920,
                altura: 1080,
                principal: true,
            },
        ])
        .expect("arranjo válido");
        assert_eq!(arranjo.len(), 2);
        assert_eq!(arranjo.primary().map(|m| m.width), Some(1920));
        assert_eq!(arranjo.monitors()[1].x, 1920);
    }

    #[test]
    fn sem_monitor_nao_ha_arranjo() {
        assert_eq!(montar(&[]), None);
    }

    #[test]
    fn a_sessao_do_teste_tem_ao_menos_uma_tela() {
        // Numa sessão interativa há tela; numa máquina de integração sem sessão pode não haver.
        if let Some(arranjo) = arranjo() {
            assert!(arranjo.primary().is_some());
        }
    }
}
