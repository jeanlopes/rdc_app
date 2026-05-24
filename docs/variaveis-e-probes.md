# Variáveis e Semantic Probes

Como o RDC serializa variáveis e como usar semantic probes para dar contexto semântico ao estado do programa.

---

## Serialização de variáveis

Toda variável retornada por `read_locals` ou `evaluate_expression` segue o mesmo formato JSON.

### Tipos escalares

```json
{ "type": "bool",   "value": true }
{ "type": "i32",    "value": -12 }
{ "type": "u64",    "value": 9999999 }
{ "type": "f64",    "value": 3.14 }
{ "type": "char",   "value": "A" }
{ "type": "()",     "value": null }
```

Tipos inteiros suportados: `i8`, `i16`, `i32`, `i64`, `i128`, `u8`, `u16`, `u32`, `u64`, `u128`, `isize`, `usize`.

### Strings

```json
{
  "type": "String",
  "value": "hello world",
  "truncated": false
}
```

Strings longas são truncadas com `"truncated": true` e um campo `"original_len"`:

```json
{
  "type": "String",
  "value": "primeiros 256 caracteres...",
  "truncated": true,
  "original_len": 10482
}
```

### Structs

```json
{
  "type": "struct",
  "type_name": "Rect",
  "fields": {
    "x": { "type": "f32", "value": 10.0 },
    "y": { "type": "f32", "value": 20.0 },
    "width":  { "type": "f32", "value": 100.0 },
    "height": { "type": "f32", "value": 50.0 }
  }
}
```

### Enums

```json
{
  "type": "enum",
  "type_name": "Option<i32>",
  "variant": "Some",
  "fields": {
    "0": { "type": "i32", "value": 42 }
  }
}
```

```json
{
  "type": "enum",
  "type_name": "Option<i32>",
  "variant": "None",
  "fields": {}
}
```

### Arrays, slices e Vec

```json
{
  "type": "array",
  "type_name": "[i32; 3]",
  "elements": [
    { "type": "i32", "value": 1 },
    { "type": "i32", "value": 2 },
    { "type": "i32", "value": 3 }
  ]
}
```

Para coleções grandes, apenas os primeiros N elementos são retornados:

```json
{
  "type": "vec",
  "type_name": "Vec<u8>",
  "len": 1024,
  "elements": [ ... ],
  "truncated": true
}
```

### Ponteiros

```json
{
  "type": "pointer",
  "type_name": "*const i32",
  "address": "0x7ff812345678",
  "pointee": { "type": "i32", "value": 99 }
}
```

Se o ponteiro for nulo ou inválido:

```json
{
  "type": "pointer",
  "type_name": "*const i32",
  "address": "0x0",
  "pointee": null,
  "error": "null pointer"
}
```

### Referências cíclicas

Quando uma struct referencia a si mesma (lista encadeada, árvore, etc.), o RDC detecta o ciclo e para de expandir:

```json
{
  "type": "struct",
  "type_name": "Node",
  "fields": {
    "value": { "type": "i32", "value": 1 },
    "next": { "type": "cyclic_ref", "address": "0x7ff8..." }
  }
}
```

### Erros de leitura

```json
{
  "type": "error",
  "message": "cannot read memory at 0x7ff812345678"
}
```

---

## Semantic Probes

Semantic probes são anotações no código-fonte que dão nomes semânticos a variáveis locais. Em vez de ver `x: -12`, o agente vê `measure_layout.remaining_width: -12`.

### Como definir uma probe

No seu código Rust, use a macro `probe!`:

```rust
fn measure_layout(ui: &Ui, available: Rect) -> LayoutResult {
    let current_x = 0.0_f32;
    let remaining_width = available.width;
    let overflow_detected = false;

    // define a semantic probe com contexto "measure_layout"
    probe!("measure_layout", current_x, remaining_width, overflow_detected);

    // ... lógica de layout ...
}
```

### Como funciona

A macro `probe!` não altera a lógica do programa. Em debug builds, ela emite um marcador no binário que o RDC usa para:

1. Identificar o contexto semântico (`"measure_layout"`)
2. Criar nomes qualificados para as variáveis (`measure_layout.current_x`)

### Lendo variáveis com probe context

Ao chamar `read_locals` (via MCP ou diretamente), passe o `probe_context`:

```json
{
  "tool": "read_locals",
  "arguments": {
    "frame_index": 0,
    "probe_context": "measure_layout",
    "max_depth": 2
  }
}
```

**Retorno sem probe context:**

```json
{
  "locals": [
    { "name": "current_x",       "type_name": "f32", "value": { "type": "f32", "value": 88.0 } },
    { "name": "remaining_width", "type_name": "f32", "value": { "type": "f32", "value": -12.0 } },
    { "name": "overflow_detected","type_name": "bool","value": { "type": "bool", "value": true } }
  ]
}
```

**Retorno com `probe_context: "measure_layout"`:**

```json
{
  "locals": [
    { "name": "measure_layout.current_x",        "type_name": "f32", "value": { "type": "f32", "value": 88.0 } },
    { "name": "measure_layout.remaining_width",  "type_name": "f32", "value": { "type": "f32", "value": -12.0 } },
    { "name": "measure_layout.overflow_detected","type_name": "bool","value": { "type": "bool", "value": true } }
  ]
}
```

O agente de IA agora sabe que `remaining_width` é negativo **e que isso significa overflow de layout** — sem precisar conhecer o codebase.

---

## Profundidade de expansão (`max_depth`)

O parâmetro `max_depth` controla quantos níveis de structs aninhadas são expandidos.

| `max_depth` | Comportamento |
|---|---|
| `1` | Apenas o valor direto (sem expandir campos de structs) |
| `2` | Expande um nível de struct |
| `3` | Expande dois níveis (padrão recomendado) |
| `5+` | Útil para estruturas de dados complexas; pode ser lento |

Campos além do limite retornam `{ "type": "opaque", "type_name": "..." }`.

---

## Boas práticas

- **Use probes em funções críticas** — não em todas as funções. O overhead de nomeação é zero em release builds.
- **Nomes de contexto devem ser únicos** — `"measure_layout"` é melhor que `"layout"` para evitar colisão entre probes.
- **`max_depth: 3`** cobre a maioria dos casos. Suba para `5` apenas quando inspecionar coleções aninhadas.
- **Para erros de serialização**, o campo `"error"` sempre estará presente no objeto da variável — nunca omitido.
