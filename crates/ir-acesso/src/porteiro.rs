//! Quem pode usar cada canal — a regra, sem sistema operacional no meio.
//!
//! A decisão fica separada de onde a credencial é lida (o ponto de escuta do serviço) e de onde a filiação a
//! grupos é consultada ([`crate::grupo`], no Linux) por um motivo prático: é a parte que precisa
//! estar certa, e uma função pura é a única coisa que dá para testar por inteiro sem montar
//! usuários e grupos de verdade numa máquina.
//!
//! # A regra
//!
//! - **root** e o **próprio dono do serviço** sempre entram: nenhum dos dois cruza fronteira de
//!   privilégio falando com um serviço que já é deles.
//! - O canal de **controle** aceita também quem pertence ao grupo do serviço.
//! - O canal do **agente** não aceita mais ninguém: ele carrega injeção de entrada
//!   ([04, §5](../../../docs/04-seguranca.md)).
//!
//! A filiação que chega aqui é a lida no banco de usuários **na hora da conexão**, e não a que o
//! processo herdou ao nascer. É isso que faz um `usermod` valer imediatamente.

// No Windows o descritor de segurança do *pipe* decide no próprio sistema, e esta regra só é
// exercitada pelos testes. Ela continua compilando lá para os testes rodarem em qualquer máquina.
#![cfg_attr(windows, allow(dead_code))]

use crate::Acesso;

/// O uid de root.
const ROOT: u32 = 0;

/// O que o porteiro decidiu sobre uma conexão.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chamada {
    /// Pode usar o canal.
    Permitida,
    /// Não pode, e aqui está quem tentou — para o registro dizer a quem liberar.
    Negada {
        /// O usuário que conectou.
        uid: u32,
    },
}

/// Quem conectou, com a filiação a grupos lida no banco de usuários na hora da conexão.
#[derive(Debug, Clone, Copy)]
pub struct Chamador<'a> {
    /// O usuário do processo que conectou.
    pub uid: u32,
    /// Os grupos a que ele pertence **agora**, segundo o banco de usuários.
    pub grupos: &'a [u32],
}

/// Decide se `chamador` pode usar um canal com este `acesso`.
///
/// `dono` é o usuário que roda o serviço; `grupo_do_servico` é o gid do grupo que dá acesso ao
/// controle, ou `None` se ele ainda não existe nesta máquina.
pub fn decidir(
    acesso: Acesso,
    chamador: &Chamador<'_>,
    dono: u32,
    grupo_do_servico: Option<u32>,
) -> Chamada {
    if chamador.uid == ROOT || chamador.uid == dono {
        return Chamada::Permitida;
    }
    let no_grupo = grupo_do_servico.is_some_and(|gid| chamador.grupos.contains(&gid));
    match acesso {
        Acesso::UsuarioInterativo if no_grupo => Chamada::Permitida,
        Acesso::UsuarioInterativo | Acesso::Restrito => Chamada::Negada { uid: chamador.uid },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DONO: u32 = 0;
    const GRUPO: u32 = 963;
    const MAXUEL: u32 = 1000;

    fn chamador(uid: u32, grupos: &[u32]) -> Chamador<'_> {
        Chamador { uid, grupos }
    }

    #[test]
    fn root_entra_nos_dois_canais() {
        for acesso in [Acesso::UsuarioInterativo, Acesso::Restrito] {
            assert_eq!(
                decidir(acesso, &chamador(ROOT, &[]), 1234, Some(GRUPO)),
                Chamada::Permitida,
                "{acesso:?}"
            );
        }
    }

    #[test]
    fn o_dono_do_servico_entra_nos_dois_canais() {
        // O serviço rodando à mão, sem privilégio, com a janela do mesmo usuário: não há fronteira
        // de privilégio nenhuma sendo cruzada.
        for acesso in [Acesso::UsuarioInterativo, Acesso::Restrito] {
            assert_eq!(
                decidir(acesso, &chamador(MAXUEL, &[]), MAXUEL, Some(GRUPO)),
                Chamada::Permitida,
                "{acesso:?}"
            );
        }
    }

    #[test]
    fn quem_esta_no_grupo_entra_no_controle() {
        let membro = chamador(MAXUEL, &[1000, 10, GRUPO]);
        assert_eq!(
            decidir(Acesso::UsuarioInterativo, &membro, DONO, Some(GRUPO)),
            Chamada::Permitida
        );
    }

    #[test]
    fn estar_no_grupo_nao_abre_o_canal_do_agente() {
        // O canal do agente carrega injeção de entrada. Pertencer ao grupo que opera a janela não
        // pode dar a ninguém o poder de digitar no prompt de elevação.
        let membro = chamador(MAXUEL, &[GRUPO]);
        assert_eq!(
            decidir(Acesso::Restrito, &membro, DONO, Some(GRUPO)),
            Chamada::Negada { uid: MAXUEL }
        );
    }

    #[test]
    fn fora_do_grupo_e_negado_e_a_recusa_diz_quem_tentou() {
        let de_fora = chamador(MAXUEL, &[1000, 10]);
        assert_eq!(
            decidir(Acesso::UsuarioInterativo, &de_fora, DONO, Some(GRUPO)),
            Chamada::Negada { uid: MAXUEL }
        );
    }

    #[test]
    fn sem_o_grupo_criado_ninguem_de_fora_entra() {
        // Um grupo que não existe não pode virar "todo mundo pode".
        let qualquer = chamador(MAXUEL, &[GRUPO]);
        assert_eq!(
            decidir(Acesso::UsuarioInterativo, &qualquer, DONO, None),
            Chamada::Negada { uid: MAXUEL }
        );
    }
}
