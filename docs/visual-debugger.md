# Visual Debugger UI

Guia completo do `apps/visual-debugger` — aplicação desktop egui para depuração manual e observação de sessões de IA.

---

## O que é

O Visual Debugger é uma janela gráfica que mostra em tempo real:

- O **código-fonte** do arquivo atual (com syntax highlighting Rust)
- A **linha ativa** destacada em amarelo
- **Breakpoints** no gutter (pontos vermelhos)
- Uma **toolbar** com 11 ações de debug + atalhos de teclado
- Uma **árvore de arquivos** para navegar no projeto
- Uma **barra de endereço** com o caminho do arquivo atual

Ele funciona de duas formas:

1. **Depuração manual** — você controla o debug via toolbar e teclado
2. **Observação de IA** — quando um agente de IA usa as ferramentas MCP, as ações dele animam os mesmos botões e highlights da UI

---

## Pré-requisitos

| Requisito | Versão mínima |
|---|---|
| Rust | stable (MSRV 1.75) |
| Windows | 10/11 x86-64 |
| Visual Studio Build Tools | 2022 (linker MSVC) |

O binário alvo **precisa ter sido compilado com informações de debug** (padrão do `cargo build`). O arquivo `.pdb` ao lado do `.exe` é obrigatório.

---

## Build

```powershell
cargo build --workspace
```

O binário fica em `target\debug\visual-debugger.exe`.

---

## Como rodar

### Sem passar binário (modo visualização de código)

```powershell
cargo run -p visual-debugger
```

O app abre sem um processo de debug. Você pode navegar pela árvore de arquivos à esquerda e clicar em qualquer `.rs` para abrir no source viewer. Quando quiser, clique em **"Attach binary…"** na toolbar para informar um executável de debug.

### Com o binário de exemplo do workspace

```powershell
cargo run -p visual-debugger -- --executable target\debug\debug-target-example.exe
```

> Se você omitir `--executable` mas o arquivo `target\debug\debug-target-example.exe` existir, o Visual Debugger o detecta automaticamente.

### Com seu próprio binário

```powershell
cargo run -p visual-debugger -- --executable "C:\caminho\para\seu_app.exe"
```

### Ajuda

```powershell
cargo run -p visual-debugger -- --help
```

---

## Interface

### Toolbar (topo)

| Ação | Atalho | Efeito |
|------|--------|--------|
| Continue | `F5` | Resume execução |
| Break All | `Ctrl+Alt+Break` | Pausa todas as threads |
| Stop | `Shift+F5` | Termina a sessão |
| Restart | `Ctrl+Shift+F5` | Reinicia o processo |
| Step Over | `F10` | Passa por cima da próxima instrução |
| Step Into | `F11` | Entra na próxima chamada |
| Step Out | `Shift+F11` | Sai da função atual |

> **Animação de pressão**: todo botão fica "pressionado" por **200ms** quando ativado, seja por clique, teclado ou ação da IA.

> Quando nenhum binário de debug está anexado, a toolbar mostra um aviso amarelo **"⚠ No debug binary attached"** e um botão **"Attach binary…"**. Você pode digitar o caminho do `.exe` na address bar e pressionar `Enter`.

### Source Viewer (centro)

- Mostra o arquivo atual com **syntax highlighting** básico Rust
- **Linha ativa** em amarelo
- **Números de linha** na margem esquerda
- **Gutter** para clicar e adicionar/remover breakpoints (ponto vermelho)
- **Scroll virtual** — arquivos com 10.000+ linhas rodam a 60fps

### File Tree (esquerda)

- Árvore do diretório raiz do projeto
- Expande/colapsa diretórios com clique
- Clica em um arquivo para abrir no source viewer
- Destaca automaticamente o arquivo ativo quando o debug muda de frame

### Address Bar (topo, abaixo da toolbar)

- Mostra o caminho absoluto do arquivo atual
- Clique para editar e digitar um caminho diretamente
- Pressione `Enter` para carregar o arquivo (`.rs` para visualização ou `.exe` para anexar como binário de debug)
- Erros (arquivo não encontrado) aparecem em vermelho inline

---

## Comportamento com IA

Quando o Visual Debugger é iniciado, ele spawna o **MCP server in-process** automaticamente. Isso significa que:

1. O agente de IA pode se conectar via MCP (stdio)
2. Toda ação da IA (`step_over`, `set_breakpoint`, etc.) é refletida na UI
3. Os botões da toolbar **animam** exatamente como se você tivesse clicado neles
4. Breakpoints e mudanças de arquivo aparecem em tempo real

---

## Edge cases tratados

| Cenário | Comportamento |
|---------|---------------|
| Binário não encontrado | Mostra `error_banner` na UI em vez de panic |
| Arquivo > 10.000 linhas | Virtual scroll mantém 60fps |
| Linha ativa fora da viewport | Scroll automático para centralizar a linha |
| Step-back não suportado pelo target | `error_banner` com mensagem amigável; não crasha |
| Múltiplas ações rápidas da IA (< 50ms) | Cada ação registra seu próprio timestamp; animação não pula |

---

## Testes

```powershell
# Testes do crate debug-session-view (tipos compartilhados)
cargo test -p debug-session-view

# Testes do app visual-debugger (tokenizer, source view, toolbar, address bar)
cargo test -p visual-debugger

# Todos os testes do workspace
cargo test --workspace
```

---

## Arquitetura resumida

```
┌─────────────────────────────────────────┐
│         apps/visual-debugger            │
│  (eframe App → toolbar, source_view,    │
│   file_tree, address_bar)               │
└─────────────────┬───────────────────────┘
                  │  DebugSessionView
                  │  (Arc<RwLock<DebugUIState>>)
                  ▼
┌─────────────────────────────────────────┐
│      crates/debug-session-view          │
│         (shared state bus)              │
└─────────────────┬───────────────────────┘
                  │
        ┌─────────┴─────────┐
        ▼                   ▼
┌───────────────┐   ┌─────────────────┐
│  Human input  │   │  AI via MCP     │
│  (mouse/key)  │   │  (in-process)   │
└───────────────┘   └─────────────────┘
```

---

## Veja também

- [Guia Rápido](guia-rapido.md) — pré-requisitos e build geral
- [Ferramentas MCP](ferramentas-mcp.md) — referência das ferramentas usáveis pela IA
