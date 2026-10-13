# Diretrizes para agentes

## Limpeza (CLEAN UP) — não deixe lixo para trás

Builds de Rust, imagens Docker e worktrees acumulam rápido e já encheram o disco. Todo agente limpa o que ele mesmo criou, sem acumular.

- **Ao terminar uma tarefa**, remova o que você gerou: worktrees (`git worktree remove <caminho>` + `git worktree prune`), branches temporárias já mescladas ou descartadas, arquivos em `tmp/`, containers e imagens de teste, binários e dumps avulsos.
- **Rust (`target/`):** não crie um `target/` por worktree ou cópia do repositório; reutilize o do projeto (ou um `CARGO_TARGET_DIR` único). Antes de um build grande, confira o espaço livre do disco; se o `target/` passar de ~10 GB ou o disco tiver menos de ~15 GB livres, rode `cargo clean` (ou `cargo clean -p <crate>`) ao fim da sessão. `cargo clean` só apaga o que se regenera: pode usar sem medo, mas não no meio de uma tarefa que ainda vai recompilar.
- **Docker:** limpe só o que você criou (`docker builder prune`, `docker image prune`, `docker rm` dos containers de teste). **Nunca** rode `docker system prune -a --volumes` sem perguntar: volumes guardam dados de banco.
- **Nunca apague** dados do usuário, volumes de banco, `.env`, `storage/`, backups ou arquivos não versionados que você não criou.
- No relatório final, diga o que foi limpo e quanto espaço liberou (quando medido); se deixou algo de propósito, diga o quê e por quê.