# Guia Rápido

Como instalar, compilar e rodar o servidor MCP do RDC no Windows.

---

## Pré-requisitos

| Requisito | Versão mínima | Como instalar |
|---|---|---|
| Rust | stable (MSRV 1.75) | `rustup default stable` |
| LLVM / clang | 14+ | [releases.llvm.org](https://releases.llvm.org) ou `winget install LLVM.LLVM`) |
| Visual Studio Build Tools | 2022 | Necessário para linker MSVC |
| Binário compilado com debug info | — | `cargo build` (sem `--release`) |

> O binário alvo **precisa ter sido compilado com informações de debug** (padrão no `cargo build`). O arquivo `.pdb` gerado ao lado do executável é obrigatório para inspeção de variáveis e breakpoints por linha de código.

---

## Build

```powershell
cargo build --workspace
```

Os binários ficam em `target\debug\`.

---

## Rodando o servidor MCP

```powershell
.\target\debug\mcp-server.exe --executable .\caminho\para\seu\binario.exe
```

O servidor fica escutando no stdio (stdin/stdout), no formato MCP (JSON-RPC). Ele está pronto para receber chamadas de um agente de IA.

---

## Rodando o Visual Debugger

### Modo visualização (sem binário)

```powershell
cargo run -p visual-debugger
```

Abre o app sem processo de debug. Você pode navegar e abrir arquivos `.rs` diretamente. Para anexar um binário depois, clique em **"Attach binary…"** ou digite o caminho na address bar.

### Com binário de exemplo

```powershell
cargo run -p visual-debugger -- --executable target\debug\debug-target-example.exe
```

> Se você omitir `--executable` mas `target\debug\debug-target-example.exe` existir, ele é detectado automaticamente.

### Com binário próprio

```powershell
cargo run -p visual-debugger -- --executable "C:\caminho\para\seu_app.exe"
```

Para um guia completo da interface, atalhos e comportamento com IA, veja [Visual Debugger UI](visual-debugger.md).

### Conectar ao Claude Desktop

Adicione ao `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "rdc": {
      "command": "C:\\workspace\\rdc_app\\target\\debug\\mcp-server.exe",
      "args": ["--executable", "C:\\caminho\\para\\seu\\binario.exe"]
    }
  }
}
```

---

## Validando a instalação

Compile o binário de exemplo incluído no workspace:

```powershell
cargo build -p debug-target-example
```

Em seguida, inicie o servidor apontando para ele:

```powershell
.\target\debug\mcp-server.exe --executable .\target\debug\debug-target-example.exe
```

Se o servidor iniciar sem erros, a instalação está correta.

---

## Executando a suíte de testes

```powershell
# Todos os testes (unitários + integração)
cargo test --workspace

# Apenas testes de integração do win-debug-bridge
cargo test -p win-debug-bridge --test '*'

# Com output de log (útil para debug)
RUST_LOG=debug cargo test -p win-debug-bridge
```

---

## Gerando a documentação dos crates

```powershell
cargo doc --no-deps --open
```

Abre a documentação gerada no browser. Para incluir itens privados:

```powershell
cargo doc --no-deps --document-private-items --open
```
