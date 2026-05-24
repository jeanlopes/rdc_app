# RDC — Wiki

Documentação de uso do RDC (Runtime Debugger Console), em português.

---

## O que é o RDC?

O RDC é uma plataforma de depuração em tempo de execução para Windows. Ele expõe um debugger completo (via Windows Debug API + PDB) de duas formas:

- **Via MCP** — 13 ferramentas consumíveis por agentes de IA (Claude, etc.)
- **Via API Rust** — o crate `win-debug-bridge` diretamente no seu código

---

## Páginas desta wiki

| Página | Conteúdo |
|---|---|
| [Guia Rápido](guia-rapido.md) | Pré-requisitos, build, e como rodar o servidor MCP |
| [Visual Debugger UI](visual-debugger.md) | Guia completo da aplicação desktop de debug |
| [Debugger Direto (sem MCP)](debugger-direto.md) | Como usar `win-debug-bridge` como biblioteca Rust |
| [Ferramentas MCP](ferramentas-mcp.md) | Referência completa das 13 ferramentas MCP |
| [Variáveis e Semantic Probes](variaveis-e-probes.md) | Como variáveis são serializadas e como criar probes |

---

## Status das features

| Feature | Descrição | Status |
|---|---|---|
| MCP + Windows Debug Bridge | 13 ferramentas MCP sobre a Windows Debug API | ✅ Completo |
| PDB, Stepping & Panic | Breakpoints por linha, inspeção de locals, stepping, panic | ✅ Completo |
| Visual Debugger UI | Aplicação egui para debug manual e observação de IA | ✅ Completo |
| egui UI Introspection | 6 ferramentas MCP para inspecionar widgets egui | 🔧 Em desenvolvimento |

---

## Arquitetura resumida

```
Agente de IA (Claude, etc.)
        │
        │  MCP / JSON-RPC
        ▼
   apps/mcp-server
        │
        │  mpsc channel (DebugCommand)
        ▼
crates/win-debug-bridge   ←── Windows Debug API (CreateProcess + WaitForDebugEvent)
        │
        │  PDB crate
        ▼
   Símbolos / Source maps  ←── arquivo .pdb gerado pelo compilador
```
