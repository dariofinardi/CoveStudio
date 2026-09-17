# Plan: Integrazione Mistral in Cove Studio

## Contesto

Cove Studio supporta già i provider OpenAI, Anthropic, Vertex AI e Ollama tramite una trait astratta `LlmProvider`. Mistral viene aggiunto come quinto provider cloud, con parità funzionale rispetto agli altri: chat completion, streaming SSE, tool calling, e configurazione via UI nella sezione Impostazioni → Modelli LLM.

Mistral è un provider cloud EU-hosted (server in Francia), compatibile con i requisiti GDPR dei clienti legali. Supporta tool calling nativo con lo stesso schema OpenAI, quindi l'integrazione è a bassa frizione rispetto alla trait esistente.

---

## Scope

| In scope | Out of scope |
|---|---|
| Provider Mistral nel backend Rust | Forge (custom model training) |
| Streaming SSE | Agents API (conversazioni stateful) |
| Tool calling | Codestral (endpoint separato, valutare in futuro) |
| UI: pill + card in Impostazioni | Fine-tuning o upload dataset |
| Persistenza config in `settings.json` | |
| Test di integrazione | |

---

## Modelli supportati

| Identificatore API | Uso consigliato in Cove Studio |
|---|---|
| `mistral-medium-3-5` | Default — document intelligence, analisi legale |
| `mistral-small-4` | Workflow leggeri, riduzione costi |
| `mistral-large-latest` | Alias stabile per il modello più capace disponibile |

Il campo modello è libero (stringa editabile dall'utente), con questi tre come suggeriti nel dropdown.

---

## Architettura backend

### 1. Struttura file

```
src-tauri/src/llm/
├── mod.rs                  # trait LlmProvider, tipi condivisi (già esistente)
├── openai.rs               # già esistente
├── anthropic.rs            # già esistente
├── vertex.rs               # già esistente
├── ollama.rs               # già esistente
└── mistral.rs              # NUOVO
```

### 2. Trait esistente (riferimento)

Il nuovo modulo deve implementare la stessa trait degli altri provider:

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn chat_completion(
        &self,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<ToolDefinition>>,
        options: CompletionOptions,
    ) -> Result<CompletionResponse>;

    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<ToolDefinition>>,
        options: CompletionOptions,
        tx: mpsc::Sender<StreamChunk>,
    ) -> Result<()>;

    fn capabilities(&self) -> ProviderCapabilities;
}
```

### 3. `mistral.rs` — struttura del modulo

```rust
// src-tauri/src/llm/mistral.rs
// Dipendenze: reqwest (MIT/Apache-2.0), serde_json (MIT/Apache-2.0)
// Nessuna dipendenza GPL/AGPL.

pub struct MistralProvider {
    api_key: String,
    model: String,
    base_url: String, // default: "https://api.mistral.ai/v1"
    client: reqwest::Client,
}

impl MistralProvider {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self { ... }
}
```

### 4. Endpoint e formato richiesta

Mistral usa il formato chat completions OpenAI-compatibile:

```
POST https://api.mistral.ai/v1/chat/completions
Authorization: Bearer {api_key}
Content-Type: application/json
```

```json
{
  "model": "mistral-medium-3-5",
  "messages": [...],
  "tools": [...],
  "tool_choice": "auto",
  "parallel_tool_calls": false,
  "stream": false
}
```

Il campo `parallel_tool_calls` va impostato a `false` per Cove Studio: i workflow legali sono sequenziali e la prevedibilità è prioritaria rispetto alla velocità.

### 5. Tool calling

Il formato è identico a OpenAI. La risposta contiene `choices[0].message.tool_calls`:

```json
{
  "tool_calls": [{
    "id": "call_abc123",
    "type": "function",
    "function": {
      "name": "cerca_normativa",
      "arguments": "{\"query\": \"art. 13 GDPR\"}"
    }
  }]
}
```

Nessuna mappatura speciale necessaria: i tipi `ToolCall` già usati per OpenAI sono riutilizzabili direttamente.

### 6. Streaming SSE

Mistral usa SSE standard con eventi `data: {...}` e termina con `data: [DONE]`, identico a OpenAI. Il parser SSE esistente è riutilizzabile senza modifiche.

### 7. `ProviderCapabilities`

```rust
fn capabilities(&self) -> ProviderCapabilities {
    ProviderCapabilities {
        supports_tools: true,
        supports_streaming: true,
        supports_vision: false, // Pixtral è un modello separato, non incluso ora
        max_context_tokens: 131_072, // mistral-medium-3-5
    }
}
```

### 8. Configurazione e Zero Data Retention

Per abilitare ZDR (Zero Data Retention) aggiungere l'header:

```
X-Mistral-No-Store: true
```

Esporre questa opzione come checkbox nella card UI ("Non archiviare le richieste sui server Mistral"). Consigliato per clienti che trattano dati personali.

---

## Configurazione persistita

Aggiungere al blocco provider in `settings.json`:

```json
{
  "providers": {
    "mistral": {
      "api_key": "",
      "model": "mistral-medium-3-5",
      "zero_data_retention": true
    }
  }
}
```

Il campo `api_key` va cifrato a riposo con lo stesso meccanismo già usato per le altre chiavi (keyring di sistema via `keyring` crate, MIT).

---

## UI: Impostazioni → Modelli LLM

### Pill provider attivo

Aggiungere pill **Mistral** accanto a OpenAI, Anthropic, Google Gemini, Locale. Ordine suggerito:

```
OpenAI | Anthropic | Google Gemini | Mistral | Locale
```

### Card configurazione Mistral

Struttura analoga alle card esistenti:

```
┌─ Mistral ────────────────────────────────────────┐
│ Chiave API  [••••••••••••••••]  [Elimina]         │
│ Modello     [mistral-medium-3-5 ▾]                │
│             ↳ suggeriti: medium-3-5, small-4,     │
│               large-latest (campo libero)         │
│ ☑ Non archiviare le richieste (Zero Data          │
│   Retention) — consigliato per dati personali     │
│                                                   │
│ ℹ Server ospitati in EU (Francia)                 │
└───────────────────────────────────────────────────┘
```

Componente: `components/settings/LLMProviderCard.svelte` — nessun nuovo componente, parametrizzare l'esistente con `provider="mistral"`.

---

## Test

### Unit test (backend Rust)

```
tests/llm/mistral_test.rs
```

| Test | Descrizione |
|---|---|
| `test_chat_completion_ok` | Mock HTTP 200, verifica mapping risposta |
| `test_chat_completion_with_tools` | Verifica parsing `tool_calls` |
| `test_stream_chunks` | Verifica SSE parsing con `[DONE]` |
| `test_zero_data_retention_header` | Verifica header `X-Mistral-No-Store` presente |
| `test_auth_error` | HTTP 401 → `MikeError::AuthError` |
| `test_rate_limit` | HTTP 429 → `MikeError::RateLimited` con retry-after |

Usare `wiremock` (MIT) per il mock HTTP server nei test, già nel dev-dependency se usato altrove.

### Test di integrazione (richiede chiave reale)

Abilitati solo con feature flag `integration-tests` e variabile d'ambiente `MISTRAL_API_KEY`:

```
cargo test --features integration-tests -- mistral::integration
```

---

## Dipendenze

Nessuna dipendenza nuova necessaria. Tutto il lavoro usa crate già presenti:

| Crate | Licenza | Uso |
|---|---|---|
| `reqwest` | MIT/Apache-2.0 | HTTP client + SSE |
| `serde_json` | MIT/Apache-2.0 | Serializzazione payload |
| `tokio` | MIT | Async runtime |
| `async-trait` | MIT/Apache-2.0 | Trait asincrona |

---

## Checklist implementazione

### Backend
- [ ] `src-tauri/src/llm/mistral.rs` — struct + `impl LlmProvider`
- [ ] `chat_completion()` — non-streaming
- [ ] `chat_stream()` — SSE con parser riusato da OpenAI
- [ ] Tool calling — parsing `tool_calls` dalla risposta
- [ ] Header `X-Mistral-No-Store` condizionale
- [ ] Gestione errori: 401, 429, 500, timeout
- [ ] Registrazione in `mod.rs` nel registry provider
- [ ] Aggiornamento `settings.json` schema
- [ ] Cifratura `api_key` via keyring

### Frontend
- [ ] Pill Mistral in `components/ui/ProviderPill.svelte`
- [ ] Card Mistral in `components/settings/LLMProviderCard.svelte`
- [ ] Dropdown modelli con tre suggeriti + campo libero
- [ ] Checkbox Zero Data Retention
- [ ] Nota EU hosting

### Test
- [ ] 6 unit test con wiremock
- [ ] Test integrazione con feature flag

---

## Stima effort

| Area | Effort |
|---|---|
| `mistral.rs` — completion + stream + tools | 3–4 h |
| Gestione errori + ZDR header | 1 h |
| Unit test | 2 h |
| UI (pill + card) | 1–2 h |
| **Totale** | **7–9 h** |

L'effort basso è giustificato dal riuso quasi totale del codice OpenAI: endpoint compatibile, stesso formato SSE, stesso schema tool calling. La differenza principale è solo l'header di autenticazione e il modello di errori.