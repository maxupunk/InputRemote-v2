# ADR-0016 — As dependências da pasta compartilhada

**Status:** aceito · **Data:** 2026-10-02 · ([log 58](../logs/58-a-pasta-que-sincroniza.md);
sob demanda nos dois sistemas: [log 59](../logs/59-a-pasta-sob-demanda.md))

## O contexto

A pasta compartilhada ([ADR-0015](0015-pastas-compartilhadas.md)) precisa de coisas que a
biblioteca padrão não dá: saber quando algo muda no disco, mostrar o seletor de pasta do sistema,
saber o fuso local para o nome da cópia de conflito e, na pasta recebida, trazer um arquivo só quando
ele é aberto — a Cloud Files API no Windows, um sistema de arquivos em espaço de usuário no Linux. A regra de [09, §7](../09-padroes-de-codigo.md)
pede um ADR para cada dependência nova.

## As decisões

| Dependência | Onde | Para quê | Por que esta |
|---|---|---|---|
| `notify` 8.2 (CC0-1.0), sem *features* padrão | `ir-sincronia` | o aviso do sistema de que algo mudou numa pasta compartilhada | é a que o ecossistema usa (rust-analyzer, `cargo watch`); por baixo é `ReadDirectoryChangesW` no Windows e `inotify` no Linux, sem *polling* |
| `rfd` 0.16 (MIT), sem *features* padrão, **só no Windows** | `ir-ui` | o seletor de pasta nativo (`IFileOpenDialog`) | sem as *features* padrão ele não traz nada além do `windows-sys`, que a janela já usa |
| `windows` 0.62, *feature* `Win32_System_Time` | `ir-sincronia`, só no Windows | `GetTimeZoneInformation`, para a hora no nome da cópia de conflito | o crate já está no projeto; a *feature* só acrescenta uma função |
| `rand` 0.8 | `ir-sincronia` | sortear o identificador de uma pasta nova | já está no projeto |
| `windows` 0.62, *features* `Win32_Storage_CloudFilters`, `Win32_Storage_FileSystem`, `Win32_System_IO`, `Win32_System_CorrelationVector`, `Win32_System_Registry`, `Foundation`, `Foundation_Collections`, `Storage`, `Storage_Provider`, `Storage_Search` | `ir-nuvem`, só no Windows | a raiz de sincronia, os marcadores e a entrega dos bytes (a mesma API do OneDrive); a entrada no painel do Explorer (`StorageProviderSyncRootManager`); as raízes registradas, lidas do registro para limpar as órfãs | o crate já está no projeto; as *features* só acrescentam as funções |
| `windows-future` 0.3 (MIT/Apache-2.0), só no Windows | `ir-nuvem` | esperar o `StorageFolder` da raiz, que a API do painel só dá assíncrono | é o par do `windows` 0.62 para `IAsyncOperation`; sem *runtime* |
| `fuser` 0.18 (MIT), sem *features* padrão, só no Linux | `ir-nuvem` | a pasta recebida sob demanda: o sistema de arquivos que mostra o cache e espera o conteúdo de um arquivo só quando ele é aberto | é a implementação do protocolo FUSE em Rust que o ecossistema usa; sem as *features* padrão ele monta pelo `fusermount3` do pacote `fuse3`, sem ligar à `libfuse` nem trazer código C |
| `libc` 0.2 | `ir-nuvem`, só no Linux | a marca de "sem conteúdo" num atributo estendido (`setxattr`), e o espaço livre (`statvfs`) | já está no projeto, por baixo de quase tudo |

## O que foi deixado de fora

- **`rfd` no Linux.** Lá o seletor é o portal do ambiente gráfico, e o `rfd` o alcança pelo `ashpd`,
  que traz um *runtime* assíncrono inteiro (`async-std` ou `tokio`) para abrir um diálogo. O
  `zenity` faz o mesmo pedido ao portal, vem com o GNOME, e o pacote o declara (`Requires: zenity`).
- **`chrono` ou `jiff` para o fuso.** No Linux, `date +%z` responde com o fuso que o sistema usa,
  horário de verão incluso; no Windows, a API do sistema. Uma biblioteca de calendário inteira para
  uma conta de segundos não se paga.
- **`libfuse` pela ligação em C** (o `fuser` com a *feature* `libfuse`). Traria a biblioteca e o
  `pkg-config` ao empacotamento para o que o `fusermount3` já faz: montar como o usuário.
- **Um pacote MSIX para a entrada no painel do Explorer.** O registro da raiz pelo
  `StorageProviderSyncRootManager` funciona sem identidade de pacote; só a lista das raízes
  registradas (`GetCurrentSyncRoots`) volta vazia — e ela é lida do registro.
- **SQLite para o índice.** Retrato em `postcard`, gravado de forma atômica, basta até as 100 mil
  entradas por pasta ([ADR-0015](0015-pastas-compartilhadas.md), alternativas rejeitadas).

## Consequências

- As licenças de todas estão na lista de `deny.toml` (MIT, Apache-2.0, CC0-1.0), inclusive as do
  que o `fuser` traz (`nix`, `zerocopy`, `parking_lot`, `page_size`, `num_enum`).
- O pacote do Linux passa a pedir `fuse3`. Sem o `fusermount3`, a pasta recebida é cópia inteira,
  como na F1.
- O ajudante das pastas (`ir-sincronia`) é o único crate que observa o disco; o motor (`ir-pasta`)
  continua puro, na lista de pureza do `xtask`.
