# Usando o Debugger Diretamente (sem MCP)

O crate `win-debug-bridge` pode ser usado como biblioteca Rust, sem nenhuma camada MCP.  
Isso é útil para automação de testes, ferramentas CLI personalizadas, ou integração em outros sistemas.

---

## Adicionando a dependência

No `Cargo.toml` do seu crate:

```toml
[dependencies]
win-debug-bridge = { path = "../crates/win-debug-bridge" }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

---

## Ponto de entrada: `WindowsDebugHandle`

Toda interação passa por `WindowsDebugHandle`. Ele é `Clone` — você pode compartilhar entre tasks Tokio sem custo.

```rust
use win_debug_bridge::thread::WindowsDebugHandle;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let handle = WindowsDebugHandle::spawn()?;
    // handle pode ser clonado e enviado para outras tasks
    Ok(())
}
```

`spawn()` cria uma thread dedicada ao sistema de debug do Windows (requisito de design — a Windows Debug API é síncrona e precisa de thread exclusiva).

---

## Iniciando um processo

```rust
use runtime_core::{DebugTarget, BreakpointKind};

let target = DebugTarget {
    executable: "C:\\caminho\\para\\binario.exe".into(),
    args: vec![],
    working_dir: None,
    env: vec![],
};

let (pid, state) = handle.launch_process(target).await?;
println!("Processo iniciado: PID={pid}, estado={state:?}");
```

O processo inicia **pausado no entry point** — nenhuma linha de código do usuário foi executada ainda.

---

## Gerenciando breakpoints

### Definir por linha de código

```rust
use runtime_core::BreakpointKind;
use std::path::PathBuf;

let bp = handle.set_breakpoint(
    BreakpointKind::SourceLine {
        file: PathBuf::from("src\\main.rs"),
        line: 42,
    },
    None, // sem condição
).await?;

println!("Breakpoint criado: id={}", bp.id);
```

### Definir por nome de função

```rust
let bp = handle.set_breakpoint(
    BreakpointKind::FunctionName("minha_funcao".to_string()),
    None,
).await?;
```

### Com condição

```rust
let bp = handle.set_breakpoint(
    BreakpointKind::SourceLine { file: "src\\lib.rs".into(), line: 10 },
    Some("x > 100".to_string()),
).await?;
```

### Listar e remover

```rust
let breakpoints = handle.list_breakpoints().await?;
for bp in &breakpoints {
    println!("{:?}", bp);
}

handle.remove_breakpoint(bp.id).await?;
```

---

## Controlando a execução

```rust
use protocol::tools::execution::ExecutionEvent;

// Continua até o próximo breakpoint ou evento
let event = handle.continue_execution().await?;

match event {
    ExecutionEvent::BreakpointHit { breakpoint_id, thread_id, location } => {
        println!("Breakpoint {breakpoint_id} atingido na thread {thread_id:?}");
        println!("Localização: {location:?}");
    }
    ExecutionEvent::Stepped { thread_id, location } => {
        println!("Step concluído em {location:?}");
    }
    ExecutionEvent::ProcessExited { exit_code } => {
        println!("Processo encerrado com código {exit_code}");
    }
    ExecutionEvent::Paused { thread_id, reason } => {
        println!("Pausado: {reason:?}");
    }
    ExecutionEvent::PanicDetected { message, location } => {
        println!("PANIC: {message}");
        println!("Em: {location:?}");
    }
    _ => {}
}
```

### Pausar um processo em execução

```rust
let event = handle.pause_execution().await?;
```

---

## Stepping (passo a passo)

Todos os métodos de stepping recebem `thread_id: Option<ThreadId>`.  
Passando `None`, o debugger usa a thread atual (a que atingiu o breakpoint).

```rust
// Próxima linha (não entra em funções chamadas)
let event = handle.step_over(None).await?;

// Entra na função chamada na linha atual
let event = handle.step_into(None).await?;

// Sai da função atual, volta ao chamador
let event = handle.step_out(None).await?;
```

---

## Inspecionando estado

### Variáveis locais do frame atual

```rust
let locals = handle.read_locals(
    None,     // thread_id: usa thread atual
    0,        // frame_index: 0 = frame mais recente
    None,     // probe_context: None = sem filtro de semantic probe
    3,        // max_depth: profundidade máxima de expansão
).await?;

for var in locals {
    println!("{}: {} = {:?}", var.name, var.type_name, var.value);
}
```

Para filtrar por semantic probe (ver [Variáveis e Probes](variaveis-e-probes.md)):

```rust
let locals = handle.read_locals(None, 0, Some("measure_layout".to_string()), 3).await?;
```

### Call stack

```rust
let frames = handle.read_stack(
    None,  // thread_id: usa thread atual
    32,    // max_frames
).await?;

for frame in &frames {
    println!("#{} {} em {}:{}", frame.index, frame.function_name, frame.file, frame.line);
}
```

### Avaliar expressão

```rust
let result = handle.evaluate_expression(
    "x + y * 2".to_string(),
    None, // thread_id
    0,    // frame_index
).await?;

println!("Resultado: {:?}", result.value);
```

### Listar threads

```rust
let threads = handle.list_threads().await?;
for t in threads {
    println!("Thread {}: {:?} — {:?}", t.id, t.name, t.stop_reason);
}
```

### Estado da sessão

```rust
let state = handle.get_state().await?;
println!("{state:?}");
// Running | Stopped | Terminated | NotStarted
```

---

## Exemplo completo: rodar até o fim

```rust
use win_debug_bridge::thread::WindowsDebugHandle;
use runtime_core::DebugTarget;
use protocol::tools::execution::ExecutionEvent;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let handle = WindowsDebugHandle::spawn()?;

    handle.launch_process(DebugTarget {
        executable: "target\\debug\\debug-target-example.exe".into(),
        args: vec![],
        working_dir: None,
        env: vec![],
    }).await?;

    loop {
        match handle.continue_execution().await? {
            ExecutionEvent::ProcessExited { exit_code } => {
                println!("Saiu com código {exit_code}");
                break;
            }
            ExecutionEvent::PanicDetected { message, .. } => {
                eprintln!("PANIC: {message}");
                break;
            }
            event => {
                println!("Evento: {event:?}");
            }
        }
    }

    Ok(())
}
```

---

## Resolução de símbolos via `PdbInfo` (avançado)

Para consultar o arquivo PDB diretamente, sem iniciar um processo:

```rust
use win_debug_bridge::pdb_info::PdbInfo;
use std::path::Path;

let pdb = PdbInfo::load(
    Path::new("target\\debug\\meu_binario.exe"),
    0x140000000, // image base (obtido do PE header)
)?;

// Endereço virtual → localização no código-fonte
if let Some(loc) = pdb.va_to_source(0x140001234) {
    println!("{}:{}", loc.file.display(), loc.line);
}

// Linha de código-fonte → endereço virtual
if let Some(va) = pdb.source_to_va(Path::new("src\\main.rs"), 42) {
    println!("VA = {va:#x}");
}

// Nome de função → endereço virtual
if let Some(va) = pdb.function_name_to_va("minha_funcao") {
    println!("VA = {va:#x}");
}

// Variáveis locais disponíveis em um endereço
let locals = pdb.locals_at_va(0x140001234);
for local in locals {
    println!("{}: {} ({} bytes)", local.name, local.type_name, local.size);
}
```
