# Ferramentas MCP

Referência completa das 13 ferramentas expostas pelo `mcp-server`.  
Todas seguem o protocolo MCP (JSON-RPC 2.0) via stdin/stdout.

---

## Índice

| Ferramenta | Categoria | Descrição curta |
|---|---|---|
| [`launch_process`](#launch_process) | Sessão | Inicia um processo sob o debugger |
| [`get_session_state`](#get_session_state) | Sessão | Retorna o estado atual da sessão |
| [`set_breakpoint`](#set_breakpoint) | Breakpoints | Define um breakpoint |
| [`remove_breakpoint`](#remove_breakpoint) | Breakpoints | Remove um breakpoint por ID |
| [`list_breakpoints`](#list_breakpoints) | Breakpoints | Lista todos os breakpoints ativos |
| [`continue_execution`](#continue_execution) | Execução | Continua a execução |
| [`pause_execution`](#pause_execution) | Execução | Pausa o processo |
| [`step_over`](#step_over) | Execução | Avança uma linha (sem entrar em funções) |
| [`step_into`](#step_into) | Execução | Entra na função chamada na linha atual |
| [`step_out`](#step_out) | Execução | Sai da função atual |
| [`read_locals`](#read_locals) | Inspeção | Lê variáveis locais do frame atual |
| [`read_stack`](#read_stack) | Inspeção | Lê o call stack |
| [`evaluate_expression`](#evaluate_expression) | Inspeção | Avalia uma expressão Rust |
| [`list_threads`](#list_threads) | Inspeção | Lista todas as threads |

---

## Códigos de erro

| Código | Significado |
|---|---|
| `-32000` | Sessão não iniciada — `launch_process` ainda não foi chamado |
| `-32001` | Processo não está parado — operação requer que esteja em `Stopped` |
| `-32002` | Breakpoint não encontrado — ID inválido ou já removido |
| `-32003` | Símbolo não encontrado — função/linha não existe no PDB |
| `-32004` | Leitura de memória falhou — endereço inválido ou sem permissão |
| `-32005` | Processo já encerrado |

---

## Sessão

### `launch_process`

Inicia um processo sob controle do debugger. O processo começa **pausado** antes de executar qualquer linha do usuário.

**Parâmetros:**

| Campo | Tipo | Obrigatório | Descrição |
|---|---|---|---|
| `executable` | string | sim | Caminho absoluto para o `.exe` |
| `args` | string[] | não | Argumentos de linha de comando |
| `working_dir` | string | não | Diretório de trabalho (padrão: diretório do `.exe`) |
| `env` | objeto | não | Variáveis de ambiente adicionais |

**Retorno:**

```json
{
  "pid": 12345,
  "state": "Stopped",
  "stop_reason": "EntryPoint"
}
```

**Exemplo:**

```json
{
  "tool": "launch_process",
  "arguments": {
    "executable": "C:\\workspace\\rdc_app\\target\\debug\\debug-target-example.exe",
    "args": [],
    "working_dir": "C:\\workspace\\rdc_app"
  }
}
```

---

### `get_session_state`

Retorna o estado atual da sessão de debug.

**Parâmetros:** nenhum

**Retorno:**

```json
{
  "state": "Stopped",
  "pid": 12345,
  "stop_reason": "Breakpoint",
  "breakpoint_id": 1
}
```

**Estados possíveis:** `NotStarted` | `Running` | `Stopped` | `Terminated`

---

## Breakpoints

### `set_breakpoint`

Define um breakpoint. Pode ser por linha de código ou por nome de função.

**Por linha de código:**

```json
{
  "tool": "set_breakpoint",
  "arguments": {
    "kind": "SourceLine",
    "file": "src\\main.rs",
    "line": 42,
    "condition": null
  }
}
```

**Por nome de função:**

```json
{
  "tool": "set_breakpoint",
  "arguments": {
    "kind": "FunctionName",
    "name": "minha_funcao",
    "condition": null
  }
}
```

**Com condição:**

```json
{
  "tool": "set_breakpoint",
  "arguments": {
    "kind": "SourceLine",
    "file": "src\\lib.rs",
    "line": 10,
    "condition": "x > 100"
  }
}
```

**Retorno:**

```json
{
  "id": 1,
  "kind": "SourceLine",
  "file": "src\\main.rs",
  "line": 42,
  "resolved_address": "0x140001234",
  "condition": null,
  "enabled": true
}
```

---

### `remove_breakpoint`

Remove um breakpoint pelo seu ID.

```json
{
  "tool": "remove_breakpoint",
  "arguments": { "id": 1 }
}
```

**Retorno:** `{}` (vazio em caso de sucesso)

---

### `list_breakpoints`

Lista todos os breakpoints ativos.

**Parâmetros:** nenhum

**Retorno:**

```json
{
  "breakpoints": [
    {
      "id": 1,
      "kind": "SourceLine",
      "file": "src\\main.rs",
      "line": 42,
      "enabled": true
    }
  ]
}
```

---

## Execução

Todas as ferramentas de execução retornam um `ExecutionEvent` descrevendo o motivo da parada.

### Estrutura `ExecutionEvent`

```json
{
  "event": "BreakpointHit",
  "breakpoint_id": 1,
  "thread_id": 1234,
  "location": {
    "file": "src\\main.rs",
    "line": 42,
    "function": "main",
    "address": "0x140001234"
  }
}
```

**Tipos de evento:**

| `event` | Quando ocorre |
|---|---|
| `BreakpointHit` | Breakpoint atingido |
| `Stepped` | Step concluído |
| `Paused` | Pausado manualmente |
| `ProcessExited` | Processo encerrou normalmente |
| `PanicDetected` | `panic!()` ou `unwrap()` em valor `None`/`Err` |
| `Exception` | Exceção de sistema (access violation, etc.) |

---

### `continue_execution`

Continua a execução até o próximo evento (breakpoint, panic, saída, etc.).

**Parâmetros:** nenhum

**Retorno:** `ExecutionEvent`

---

### `pause_execution`

Pausa o processo em execução.

**Parâmetros:** nenhum

**Retorno:** `ExecutionEvent` com `event: "Paused"`

---

### `step_over`

Avança uma linha de código. Se a linha contém uma chamada de função, ela é executada por completo sem entrar nela.

```json
{
  "tool": "step_over",
  "arguments": { "thread_id": null }
}
```

`thread_id: null` usa a thread que está pausada no momento.

**Retorno:** `ExecutionEvent` com `event: "Stepped"`

---

### `step_into`

Avança uma linha. Se a linha contém uma chamada de função, entra nela e para na primeira instrução.

```json
{
  "tool": "step_into",
  "arguments": { "thread_id": null }
}
```

---

### `step_out`

Executa até o retorno da função atual e para no chamador.

```json
{
  "tool": "step_out",
  "arguments": { "thread_id": null }
}
```

---

## Inspeção

### `read_locals`

Lê as variáveis locais do frame especificado.

```json
{
  "tool": "read_locals",
  "arguments": {
    "thread_id": null,
    "frame_index": 0,
    "probe_context": null,
    "max_depth": 3
  }
}
```

| Campo | Descrição |
|---|---|
| `thread_id` | `null` = thread atual |
| `frame_index` | `0` = frame mais recente (topo da pilha) |
| `probe_context` | Prefixo de semantic probe para filtrar (ex: `"measure_layout"`) |
| `max_depth` | Profundidade máxima de expansão de structs aninhadas |

**Retorno:**

```json
{
  "locals": [
    {
      "name": "x",
      "type_name": "i32",
      "value": { "type": "i32", "value": 88 }
    },
    {
      "name": "s",
      "type_name": "String",
      "value": {
        "type": "String",
        "value": "hello",
        "truncated": false
      }
    }
  ]
}
```

Para o formato completo de serialização de variáveis, veja [Variáveis e Probes](variaveis-e-probes.md).

---

### `read_stack`

Lê o call stack da thread especificada.

```json
{
  "tool": "read_stack",
  "arguments": {
    "thread_id": null,
    "max_frames": 32
  }
}
```

**Retorno:**

```json
{
  "frames": [
    {
      "index": 0,
      "function_name": "minha_funcao",
      "file": "src\\lib.rs",
      "line": 42,
      "address": "0x140001234",
      "module": "meu_binario.exe"
    },
    {
      "index": 1,
      "function_name": "main",
      "file": "src\\main.rs",
      "line": 10,
      "address": "0x140000abc",
      "module": "meu_binario.exe"
    }
  ]
}
```

---

### `evaluate_expression`

Avalia uma expressão Rust no contexto do frame especificado.

```json
{
  "tool": "evaluate_expression",
  "arguments": {
    "expression": "x + y * 2",
    "thread_id": null,
    "frame_index": 0
  }
}
```

**Retorno:**

```json
{
  "value": { "type": "i32", "value": 194 },
  "type_name": "i32"
}
```

---

### `list_threads`

Lista todas as threads do processo.

**Parâmetros:** nenhum

**Retorno:**

```json
{
  "threads": [
    {
      "id": 1234,
      "name": "main",
      "stop_reason": "Breakpoint",
      "current_location": {
        "file": "src\\main.rs",
        "line": 42,
        "function": "main"
      }
    },
    {
      "id": 5678,
      "name": "tokio-runtime-worker",
      "stop_reason": null,
      "current_location": null
    }
  ]
}
```
