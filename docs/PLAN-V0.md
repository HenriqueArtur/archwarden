# PLAN-V0 — plano de execução do v0

Documento vivo. Registra **o que vai ser feito**, **em que ordem**, e — depois
de cada step — **o que realmente aconteceu**.

Escopo definido por [`ROADMAP.md`](ROADMAP.md) §v0. Arquitetura por
[`ARCHITECTURE.md`](ARCHITECTURE.md). Decisões por [`DECISIONS.md`](DECISIONS.md).

---

## Protocolo deste arquivo

1. Nenhum step começa sem estar escrito aqui.
2. Cada step tem: **Objetivo**, **Tarefas**, **Pronto quando**, e um bloco
   **Registro** vazio.
3. Ao concluir um step, preencher o **Registro** com:
   - data e hora de início e fim (`YYYY-MM-DD HH:MM -03`);
   - se saiu como esperado (`✅`), com desvio (`⚠️`), ou não saiu (`❌`);
   - se houve desvio: o que era esperado, o que foi feito no lugar, e por quê.
     Uma ou duas frases. Não é post-mortem, é rastro.
4. Desvio que muda uma decisão travada → atualizar a tabela de decisões **e**
   abrir entrada em `DECISIONS.md` se for load-bearing.
5. Steps são sequenciais salvo quando marcados `∥` (paralelizável).

Estados: `⬜ não iniciado` · `🟦 em andamento` · `✅ concluído` · `⚠️ concluído com desvio` · `🚫 bloqueado`

---

## Decisões travadas

Fechadas em 2026-07-24. Referência: seção "Under-specified" da análise inicial.

| # | Assunto | Decisão |
|---|---|---|
| D1 | Store do cache | **`redb` 4.x.** `sled` descartado (ainda em `1.0.0-alpha.124`) |
| D2 | `dashmap` | **Cortado do v0.** Pipeline é `par_iter().map().collect()`, sem estado compartilhado. Reavaliar se algum estágio provar necessidade |
| D3 | Motor de regex | **`regex` 1.x** (linear, sem ReDoS). Sem lookahead/lookbehind/backreference. `config validate` deve emitir erro explicativo quando o usuário tentar |
| D4 | Semântica de `roots` | **`roots` seleciona diretórios**; cada regra declara o que olha dentro. `X/**` inclui `X`. Recursão vive no glob. Exige corrigir o exemplo de `naming` em `CONFIG.md:123` (`use-cases/*/*` → `use-cases/*`) |
| D5 | Escape hatch `_` | **Configurável** via `skip_dirs { prefixes, globs, scope }`. Default `prefixes:["_"], scope:"structure"`. `scope:"walk"` disponível, com aviso do doctor quando coexiste com `import-boundary` |
| D6 | Severidade | **Declaração de escopo mais específico vence**, dentro de uma mesma regra. `warn_subfolders` emite `warning` independente do `level`. **Cross-rule: findings seguem independentes** — regra profunda não rebaixa regra rasa. **`ignore` global sempre vence `roots`**; doctor avisa |
| D7 | `extends` npm | **`oxc_resolver` no `archwarden-config`.** Cobre npm, yarn classic, pnpm e yarn PnP. DAG fica `config → resolver → core` (acíclico); o custo é de cronograma — `archwarden-resolver` sai do M5 e entra no M1 com config de resolução de pacote apenas. Local vence preset em escalares; preset não declara `root`; `disable: [rule-id]` remove regra herdada |
| D8 | `signature_hint` | Adicionar `must_export.signature_hint` como campo **puramente documental**, nunca verificado |
| D9 | `must_export.kind` | **Tags por export**, `kind` aceita string ou lista, casa se carregar qualquer tag. `arrow` ≠ `function`. `export default` não satisfaz regra com nome. Re-export só casa `any`/`reexport` |
| D10 | Extensões no walk | Classificação explícita `Source` / `Spec` / `Other`. **`.md` entra depois** — anotado como follow-up pós-v0 |
| D11 | `ignore_files` | Vira **glob**, não path exato |
| D12 | Escopo do `check` | Repo inteiro, mesmo rodando de subdiretório. É o motivo de o cache existir |
| D13 | Budget de performance | Declarar máquina de referência no `ROADMAP.md`. Baselines de `criterion` são por máquina |
| D14 | Shape do `import-boundary` (C2) | **`graph` some.** Boundary vira regra normal com `type: "import-boundary"`. Campo de escopo chama **`from`** (só nesta regra), com semântica idêntica a `roots` de D4 — seleciona diretórios, importador é qualquer arquivo dentro. Config ganha array `rules` no topo, irmão de `modules`; regra sem módulo sai como `[*]` no output. Todo campo de glob aceita string **ou** array. Internamente `from` e `roots` viram o mesmo `CompiledScope` |

### Ainda bloqueia

Nada. Todas as decisões de design do v0 estão travadas.

### Correções de doc pendentes (C1–C9)

Sair num commit de docs **antes** do M0, para as docs pararem de se contradizer.

| | Onde | O quê |
|---|---|---|
| C1 | `ARCHITECTURE.md:138,214` | Watch é v1, não v0. ROADMAP vence |
| C2 | `CONFIG.md:70` vs `:158-176` vs `AGENT-INTEGRATION.md:109` | Três descrições incompatíveis do `import-boundary`. Resolvido por D14: reescrever `CONFIG.md:157-177` sem `graph`, documentar `from` e o array `rules` de topo |
| C3 | `RULES.md:84` vs `CONFIG.md:152` | `require_non_empty_spec` aceita `describe`? Proposta: **não** |
| C4 | `ARCHITECTURE.md:131` | Chave de cache precisa de `resolution_epoch` (hash de `tsconfig*.json` + `package.json` + lockfile) |
| C5 | `ARCHITECTURE.md:131` | Separar `facts[content_hash]` de `findings[content+rules+epoch]` |
| C6 | `AGENT-INTEGRATION.md:182` | `check --file` deve reportar `"skipped": [...]`, nunca pular regra em silêncio |
| C11 | `AGENT-INTEGRATION.md:145` | Shape do `scaffold` precisa de `filename_patterns` e `allowed_subfolders` |
| C12 | `ARCHITECTURE.md:252` | `agent-guide` não pode usar `describe_expectation` — ela é por caminho |
| C16 | `CONFIG.md:333` | Três checagens do doctor já são erro duro desde o M1 |
| C15 | `AGENT-INTEGRATION.md:168` | Hook recebe JSON no stdin, não `$CLAUDE_FILE_PATH` |
| C14 | `CONFIG.md` | Campo desconhecido na config era ignorado em silêncio; agora é erro |
| C13 | `AGENT-INTEGRATION.md:180` | `check --file` roda boundary rules; não existe "cold-cache" a pular |
| C7 | `.gitignore:8` | Ignorar `.archwarden/cache/`, não `.archwarden/` inteiro |
| C8 | `.gitignore:5` | **Remover `Cargo.lock`** — workspace com binário commita o lock |
| C9 | `ARCHITECTURE.md:147` | `ignore` não usa rayon; tem threadpool próprio |

---

## Convenções de engenharia

### TDD é obrigatório

Ciclo por unidade de trabalho:

1. Teste primeiro, começando por **uma frase em prosa** descrevendo o comportamento
   (mesmo processo do clean-room em `TESTING.md:73`, aplicado a todo teste).
2. Rodar e ver falhar **pelo motivo certo**. Falhar por não compilar não é red.
3. Implementar o mínimo.
4. Refatorar no verde.

Verificação objetiva, não sistema de honra:

- **`cargo-mutants`** — injeta bugs e checa se a suíte pega. Meta: zero
  sobreviventes em `archwarden-rules`, `archwarden-config`, `archwarden-core`.
  Nightly no CI; por crate durante o desenvolvimento.
- **`cargo-llvm-cov`** — piso de **99% de linha + 100% de função** no
  `archwarden-core`, **95% de linha** no workspace, meta 100%. Números
  fixados pelo Henrique em 2026-07-25.

  Chegar a 100% num crate de lógica pura significa, em boa parte, apagar
  branch defensivo que nenhuma entrada alcança — o que é uma melhora, mas é
  mudança de código motivada por métrica e precisa ser revisada como tal.

  **O piso de linha é 99 e não 100 por limitação da ferramenta, não por
  concessão.** O resumo do `cargo-llvm-cov` acusa 1 linha descoberta no
  `glob.rs` que os relatórios detalhados dele mesmo — lcov, JSON e HTML —
  mostram como coberta. É fantasma em código expandido por macro. Investigado
  a fundo (derive `Default` removido, instanciação genérica variada, `Debug`,
  `Clone` e `source()` cobertos): o número não se moveu, mas a caça produziu
  testes reais que ficaram. O piso de **função em 100%** é a metade mais
  rígida do par: apagar um teste derruba cobertura de função na hora, o que um
  piso de linha em 99 absorveria.
- **Armadilha do `insta`:** snapshot esperado é escrito **à mão antes**
  (inline snapshot). `cargo insta review` só para revisar mudança intencional
  em snapshot existente — nunca para criar o primeiro.

**Convenção: não usar `let ... else { panic!() }` para destrinchar erro em
teste.** Esse braço nunca executa quando o teste passa, então é linha morta que
nenhuma execução alcança e que derruba o piso de cobertura. Bateu quatro vezes
até virar regra. As alternativas:

- `assert_eq!` contra o valor de erro inteiro — de quebra fixa a frase exata
  que o usuário lê, em vez de checar `contains`;
- uma função auxiliar que devolve `Option`, com um teste que exercita o braço
  `None`. Aí o caminho negativo é comportamento testado, não código morto.

### Lints e ferramental

`[workspace.lints]` na raiz; cada crate herda com `[lints] workspace = true`.

| Lint | Nível | Escopo |
|---|---|---|
| `unsafe_code` | `forbid` | todos |
| `clippy::unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented` | `deny` | libs |
| `clippy::indexing_slicing` | `deny` | libs |
| `clippy::print_stdout`, `print_stderr` | `deny` | libs (só `cli` fala com terminal) |
| `clippy::dbg_macro` | `deny` | todos |
| `clippy::pedantic` | `warn` | todos |
| `missing_docs`, `unreachable_pub` | `warn` | libs |

Testes e o crate `cli` ficam liberados dos `deny` de panic.

Aprovado por Henrique em 2026-07-24. O custo dos `deny` de `unwrap`/`expect`
recai sobre quem escreve, não sobre a revisão.

CI: `fmt --check` · `clippy -D warnings` · `nextest run` · `llvm-cov` ·
`deny check` · `machete` · `typos` · `doc -D warnings` · build no MSRV 1.96.
`mutants` no nightly.

### Design

- Erros: `thiserror` nas libs (enum tipado por crate), `miette` no binário.
  Nunca `Box<dyn Error>` em API pública.
- Newtypes em vez de `String` solta: `RuleId`, `ContentHash`, `RepoRelPath`.
- Parse, don't validate: `CompiledRule` só existe se glob e regex compilaram.
- `#[non_exhaustive]` em enums públicos que vão crescer.
- Zero `async`. `rayon` cobre o paralelismo.
- `[workspace.dependencies]` — versão num lugar só.
- Doctests em todo exemplo de `///`.

---

## Steps

### M0 — Esqueleto do workspace `⚠️`

**Objetivo:** `cargo build` e o CI inteiro verdes num workspace vazio, para que
nenhuma convenção precise ser retrofitada depois.

**Tarefas**
- Commit de docs com C1–C9.
- `Cargo.toml` raiz: `[workspace]`, `[workspace.dependencies]`, `[workspace.lints]`.
- 8 crates vazios: `core`, `config`, `parser`, `resolver`, `rules`, `cache`,
  `engine`, `cli`. (`lsp` fica para v1; `engine` toma seu lugar — ver
  divergência registrada abaixo.)
- `rust-toolchain.toml` (1.96, rustfmt + clippy), `rustfmt.toml` mínimo.
- `deny.toml` com a allowlist de licenças do ADR#11.
- CI: workflow com todos os jobs da tabela acima.
- `.gitignore` corrigido (C7, C8).

**Pronto quando:** CI verde em PR, incluindo `deny check` e `llvm-cov`
(trivialmente, sem código).

**Divergência conhecida vs `ARCHITECTURE.md:59-68`:** a doc lista `archwarden-lsp`
entre os 8 crates, mas o marca como post-v1, e não prevê onde o pipeline mora.
Se a orquestração ficar no `cli`, o `lsp` de v1 teria que depender do crate de
binário. Substituído por `archwarden-engine`; `lsp` entra em v1. Total continua 8.

**Registro** — 2026-07-24, 18:40 → 20:34 -03 · `⚠️ concluído com desvio`

Tudo o que estava planejado foi entregue e verificado localmente. Sete desvios,
nenhum bloqueante.

*Verificado localmente, todos exit 0:* `cargo build`, `fmt --check`,
`clippy --workspace --all-targets -D warnings`, `test --workspace`,
`doc -D warnings`, `deny check`, `machete`, `typos`, `llvm-cov`, e os dois
binários rodam.

**1. Escopo ampliado de C1–C9 para C1–C9 + D1–D14.** Esperado: só as nove
correções de conflito. Feito: também aterrissei nas docs as decisões que mudam
a superfície pública (`from` na boundary, `skip_dirs`, tabela de `kind`,
`ignore_files` como glob, `signature_hint`, `extends` via resolver, limitação
do `regex`, máquina de referência). Motivo: sem isso `CONFIG.md` e `RULES.md`
ficariam documentando o oposto do que o `PLAN-V0.md` decidiu, e o M1 escreveria
tipos contradizendo a própria doc de referência.

**2. Lints verificados por sonda, não assumidos.** Não estava no plano. Criei
um `lints_probe.rs` temporário com `unwrap()` em código de lib e outro dentro
de `#[cfg(test)]`. Confirmado que `clippy::unwrap-used` dispara só no primeiro,
que `unreachable_pub` e `dead_code` disparam, e que `allow-unwrap-in-tests`
funciona. Sonda removida. Um lint configurado e nunca exercido não é garantia
de nada.

**3. Piso de cobertura NÃO ligado.** Esperado: CI verde "incluindo `llvm-cov`".
Feito: o job roda e reporta, mas sem `--fail-under-lines`. Motivo: no M0 não
existe código de biblioteca — as únicas linhas mensuráveis são dois `main()`,
e a cobertura é 0% por construção, não por falta de teste. Piso contra isso é
teatro. **Ligar no M1** (70% workspace, 90% em `core`/`config`/`rules`), que é
quando passa a existir código a medir. Anotado como comentário no `ci.yml`.

**4. `--ignore-filename-regex` precisa ser sem âncora.** `'^xtask/'` casa nada:
o `llvm-cov` aplica o regex contra caminho absoluto. Corrigido para `'xtask/'`
e verificado que o `xtask` some do relatório.

**5. `deny.toml` é mais largo que o ADR#11 — virou o ADR#12.** O ADR#11 fixa
MIT/Apache-2.0/BSD-2/BSD-3/0BSD para **fixtures** (dados de teste copiados com
atribuição; código de teste é sempre clean-room). Para **dependências**
adicionei `Unicode-3.0`, `ISC` e `Apache-2.0 WITH LLVM-exception` — sem eles
nenhuma árvore Rust compila. Copyleft continua fora das duas listas.
Escrito como ADR#12 porque as duas listas foram confundidas durante a revisão
deste próprio step: são ambas "lista de licenças aceitas" e não têm relação.

**6. Dois arquivos não previstos:** `.cargo/config.toml` (alias `cargo xtask`)
e `_typos.toml`. O `typos` quebrava em `PnP` (quebra em "Pn" → sugere "On") e
no português do `PLAN-V0.md`. Ambos configurados com justificativa inline.

**7. `cargo-nextest` exige `--locked`.** Falhou na primeira instalação com um
`compile_error!` deliberado do próprio nextest: ele recusa `cargo install` sem
`--locked`. Resolvido com `cargo install --locked cargo-nextest`. Registrado
aqui porque a mensagem só aparece no fim de uma compilação longa e é fácil
diagnosticar errado como problema de plataforma.

**8. `--no-tests=pass` adicionado ao job `test`, temporariamente.** Descoberto
só depois de eu declarar o M0 pronto: o `nextest` trata "zero testes" como
erro, então o job ficaria vermelho num workspace vazio — contra a definição de
pronto deste próprio step. A flag é muleta de M0 e **sair dela é tarefa
explícita do M1**, não comentário no YAML: com TDD obrigatório, "nenhum teste
rodou" tem que quebrar o build.

**Não verificado:** os workflows do GitHub Actions. Não há como executá-los
nesta VM; a sintaxe foi escrita com cuidado mas só o primeiro push confirma.

**Também confirmado:** as 24 versões de crate fixadas em
`[workspace.dependencies]` co-resolvem — 233 pacotes, zero conflito (testado
num crate descartável, fora do repo).

**Questão aberta:** o `PLAN-V0.md` está em português enquanto `README.md` e
todas as outras docs estão em inglês, num repo público com distribuição npm e
licença dupla. Vale decidir se ele migra para inglês ou se fica como documento
interno. Hoje está excluído do `typos` por causa disso.

---

### M1 — Core + config

Dividido em dois na execução, a pedido do Henrique em 2026-07-25: o step
original era materialmente maior que o M0 e entregaria um diff grande demais
para uma revisão só. M1a fecha o `archwarden-core`; M1b faz a fatia vertical
até o CLI. O critério de pronto do M1 completo é o do M1b.

---

#### M1a — `archwarden-core` completo `⚠️`

**Objetivo:** todos os tipos e traits que o resto do workspace consome, sem
nenhuma dependência interna.

**Tarefas**
- ✅ `scope`: matcher de path implementando D4 (`roots`/`from`).
- ✅ `template`: transforms de caso e `{{pascal(name)}}` via `heck`.
- ✅ `ids`: newtypes `RuleId`, `ModuleId`.
- ✅ `path`: `RepoRelPath` — garante no tipo que o caminho é relativo à raiz do
  repo. Confusão relativo-vs-absoluto é fonte clássica de bug em linter.
- ✅ `hash`: `ContentHash` (`blake3`), base das duas chaves de cache.
- ✅ `level`: `Level` (`error`/`warning`) — ADR#1.
- ✅ `facts`: `FileFacts`, `ImportFact`, `ExportFact` (com as tags de D9),
  `CallFact`. Já `Serialize`/`Deserialize`, porque o M4 vai persistir isso.
- ✅ `finding`: `Finding` carregando `Observed` + `Expectation` estruturados, não
  `String`. É o que faz o `explain` melhorado de v1 não virar refactor.
- ✅ `traits`: `Parser`, `Resolver`, `RuleEngine` (com `describe_expectation`).

**Pronto quando:** `archwarden-core` compila sem dependência interna alguma,
cobertura ≥ 90%, e `cargo mutants -p archwarden-core` sai com zero
sobreviventes.

**Registro** — 2026-07-24 21:00 → 2026-07-25 09:50 -03 · `⚠️ concluído com desvio`

Critério de pronto atingido: nove módulos, zero dependência interna,
**84 testes**, **98,7% de cobertura**, **131 mutantes / 94 mortos / 0
sobreviventes**. Todos os comandos do CI em 0.

**1. Step dividido em M1a/M1b** a pedido do Henrique, já registrado acima.

**2. `facts` não passou por fase vermelha.** Nos outros módulos escrevi teste →
stub → vermelho por asserção → implementação. Em `facts` a lógica é um bitset
mecânico e escrevi implementação e teste juntos; os 12 testes passaram de
primeira. Não é TDD. O `cargo-mutants` cobriu a lacuna depois (ver 4), mas fica
registrado como desvio do processo, não como acerto.

**3. A tag serde do `Expectation`/`Observed` é `type`, não `kind`.** Descoberto
por erro de compilação: `RequiredExport` já tem um campo `kind`, e o
`AGENT-INTEGRATION.md:149` documenta esse nome no JSON do `scaffold`. Quem cedeu
foi a tag. `type` também casa com o discriminador que o próprio config usa.

**4. `cargo-mutants` acusou 3 sobreviventes em `facts`; só 1 era buraco real.**
Os outros dois eram **mutantes equivalentes** — `1 << 0` e `1 >> 0` dão o mesmo
valor, e `ExportTags::none()` era indistinguível de `Default::default()` porque
eu derivava os dois. Nenhum teste consegue matar mutante equivalente. Em vez de
forçar teste ou suprimir, tirei a ambiguidade do código: bits viraram constantes
hex explícitas e o derive `Default` saiu (`none()` é o construtor nomeado de um
conjunto; ter os dois era API redundante). O buraco real era `|` virando `^` em
`with()`, que só difere ao adicionar uma tag já presente — teste de
idempotência adicionado.

**5. `///` em `pub mod` no `lib.rs` quebrava o rustdoc.** Com `///` na
declaração **e** `//!` dentro do módulo, o rustdoc concatena os dois e resolve
todos os links intra-doc no escopo externo, onde os tipos do módulo não existem.
`cargo doc -D warnings` só acusou quando o primeiro `[`FileFacts`]` apareceu.
`lib.rs` reescrito sem os `///`, com o motivo em comentário para não voltar.

**Uma decisão que vale confirmar:** `Expectation` e `Observed` já têm todas as
variantes das cinco categorias de regra, incluindo as de M5/M6 que ainda não
têm engine. São `#[non_exhaustive]`, então adicionar depois não quebra nada.
Fiz assim porque as variantes vêm do `RULES.md`, que é especificação, não
chute — mas se alguma categoria mudar de forma até lá, essas variantes mudam
junto.

**6. Cobertura elevada para 100% no core, a pedido do Henrique** (2026-07-25,
depois de o M1a já estar aprovado). Estava em 98,66%. As lacunas eram de três
naturezas diferentes, e só uma se resolvia escrevendo teste:

- *Teste faltando de verdade* — `as_path()`, e o caminho em que a regra é
  satisfeita e nada é reportado. Testes adicionados.
- *Braço `panic!` de `let ... else` dentro de teste* — nunca executado porque o
  teste passa. Reescritos como `assert_eq!` contra o erro inteiro, o que de
  quebra passou a fixar a frase exata que o usuário lê em vez de checar
  `contains`.
- *Branch de erro inalcançável em código de produção* — dois casos. Em
  `parse_hex`, dois `map_err` que nenhuma entrada alcança, porque comprimento e
  alfabeto já foram validados antes; substituídos por uma função `hex_value`
  total. Em `Scope::compile`, o erro do `GlobSetBuilder::build()`, impossível
  porque cada glob já foi validado individualmente; resolvido trocando
  `GlobSet` por `Vec<GlobMatcher>`, cujo construtor é infalível.

Vale registrar o padrão: **piso de 100% não se atinge só escrevendo teste.**
Boa parte do caminho foi apagar código defensivo que nenhuma entrada alcança —
o que é uma melhora, mas é uma mudança de código motivada por uma métrica, e
precisa ser olhada como tal. A troca `GlobSet` → `Vec<GlobMatcher>` é a de mais
consequência e está justificada em comentário no `scope.rs`: escopo de regra
tem um ou dois padrões, onde as duas estruturas se equivalem.

Cobertura **de linha** ficou em 100% no core; **de região** em 99,66% (7
regiões), resíduo de curto-circuito e instanciação de genéricos que o
`llvm-cov` conta separado. O piso do CI é `--fail-under-lines`.

---

#### M1b — `config` + `resolver` mínimo + CLI `⚠️`

**Objetivo:** a fatia vertical — do JSON no disco até um comando que responde.

**Tarefas**
- ✅ `config`: tipos de wire format com `Deserialize` + `JsonSchema` — enum
  `Rule` de 5 variantes (D14), array `rules` de topo + `modules[].rules`,
  helper `OneOrMany` para todo campo de glob.
- ✅ `config`: discovery subindo do CWD (ADR#4), `--config` override.
- ✅ **`resolver`: `PresetResolver` sobre `oxc_resolver`**, configurado só para
  resolução de preset. Antecipado do M5 por causa de D7 — ver nota abaixo.
- ✅ `config`: `extends` — path relativo + pacote npm via `resolver`, merge,
  escalares (local vence), `disable`, erro se preset declara `root`, detecção
  de ciclo, id duplicado.
- ✅ `config`: lowering para `core::CompiledConfig` (compila globs e regexes,
  e confere que todo template referencia grupo de captura que existe).
- ✅ `core`: `Pattern` — regex compilado, com detecção e mensagem explicativa
  para lookahead, lookbehind e backreference (D3).
- ✅ `xtask gen-schema` → `schema/v0.json`, com `check-schema` no CI para o
  schema commitado não sair de sincronia com os tipos.
- ✅ `cli`: `clap`, `archwarden config validate`, exit codes 0/1/2, render
  `miette`. Crate reestruturado com `lib.rs` testável e `main.rs` fino.
- ✅ Tier 1 em tudo. `proptest`: config loading nunca panica (`TESTING.md:150`).
- ✅ **`--no-tests=pass` removido** do job `test`. A muleta do M0 saiu assim
  que passou a existir teste.
- ✅ **Piso de cobertura ligado** no job `coverage`, com os números que o
  Henrique fixou em 2026-07-25: **100% no `archwarden-core`**, **95% no
  workspace** (meta 100%). Dois invocações do `llvm-cov`, uma por piso.

**Pronto quando:** `archwarden config validate` roda contra config válido e
inválido com exit code e mensagem `miette` corretos; schema gerado valida os
exemplos do `CONFIG.md`; `extends` resolve preset em npm, pnpm e yarn PnP.

**Registro** — 2026-07-25 09:55 → 11:39 -03 · `⚠️ concluído com desvio`

Critério atingido. **254 testes**, workspace em 98,8%, core em 100% de linha e
função, zero mutantes sobreviventes em `core`, `config` e `resolver`. Todos os
comandos do CI em 0.

Quatro coisas que testes pegaram e que eu tinha feito errado:

1. **`--config` relativo resolvia contra o CWD do processo**, não contra o
   diretório passado pro `run`. Só apareceu porque `run` recebe o diretório em
   vez de ler o ambiente — que é exatamente por que essa estrutura foi
   escolhida.
2. **Afirmei em comentário uma proteção que não existia:** `extensions:
   [".json"]` no resolver não impede preset em JavaScript, porque essa lista só
   vale para specifier sem extensão. Virou checagem em código.
3. **`main.rs` nunca tinha rodado em teste.** As 29 linhas do binário eram
   verificadas só por smoke manual. Agora 8 testes sobem o processo.
4. **`typos` acusou um erro de grafia intencional** num teste (`funktion`).
   Trocado por `callable` — palavra real que simplesmente não é um export kind,
   o que testa o mesmo caminho sem brigar com o corretor.

**Desvios registrados em detalhe acima:** piso de cobertura de linha do core
baixado de 100 para 99 por limitação do `cargo-llvm-cov` (compensado com piso
de função em 100%); `Zlib` adicionado ao `deny.toml` por causa do `foldhash`
sob o `oxc_resolver`.

**Continua aberto:** o caret do `miette` na linha errada para erros de
`Deserialize` manual (descrito abaixo). Não foi decidido se vira step próprio
ou se fica pro M8.

**Caret do `miette` — resolvido em 2026-07-25 (opção B).**

O problema era maior do que eu tinha descrito. Medi as quatro classes de erro
e **três das quatro têm span errado**, não só as vindas de `Deserialize`
manual:

| Erro | Caret do `serde_json` |
|---|---|
| variante desconhecida (`"type":"graf"`) | ✅ correto |
| tipo errado (`"id": 42`) | ❌ errado |
| campo faltando | ❌ errado |
| validação nossa (`RuleId` inválido) | ❌ errado |

E as quatro são `Category::Data`, então `classify()` **não** separa as boas das
ruins. Erros `Syntax`/`Eof` medi separadamente: esses apontam certo, porque o
parser quebra exatamente ali.

Opções levantadas: **(A)** caret só para `Syntax`, **(B)** A + caminho do campo
via `serde_path_to_error`, **(C)** B + AST com spans (`jsonc-parser`) para
caret exato em tudo, **(D)** nada. Henrique escolheu **B agora, C no M8**.

Implementado: parsing passa por `serde_path_to_error`, o caret sai para erro de
schema e fica para erro de sintaxe, e a mensagem ganha o caminho do campo. A
posição que o `serde_json` anexa ao próprio texto também é removida — era o
mesmo número não confiável, só que em prosa.

    at `rules[1]`: rule id `bad rule` contains ` `; allowed characters are
    letters, digits, `-`, `_`, `.` and `/`

**Limitação medida:** para enum com tag interna (o nosso `Rule`), o caminho
para no índice — `rules[1]`, não `rules[1].id`. O serde bufferiza o conteúdo da
variante e perde o nome do campo. `rules[1]` já é o desambiguador que importa.

**C fica como tarefa do M8**, junto com o `config doctor`, que vai precisar de
span para "campo desconhecido `allowed_subfolder`, você quis dizer
`allowed_subfolders`?" de qualquer forma.

**Nota sobre o DAG (D7):** `config` depende de `resolver`, que depende só de
`core` — acíclico. O crate `archwarden-resolver` nasce aqui com a configuração
mínima de resolução de pacote; a configuração TS-aware (`tsconfig.paths`,
extensões, condicionais de `exports`) é do M5. Mesmo crate, duas configurações,
entregues em momentos diferentes.

**Registro**
> _(pendente)_

---

### M2 — Walk + `structure` + `spec-pair` `⚠️`

**Objetivo:** substituir o `check-structure.ts` do Flowmaatik. Sem parser, sem
cache, sem grafo.

**Tarefas**
- `engine`: walk com `ignore::WalkBuilder` (respeita `.gitignore` + `ignore`
  do config), `skip_dirs` de D5, classificação `Source`/`Spec`/`Other` (D10).
- `engine`: hash de conteúdo `blake3` já no walk (prepara M4).
- `engine`: matcher roots→regra, usando o de `core`.
- `rules`: engine `structure` (`allowed_subfolders`, `warn_subfolders` com D6,
  `recurse_into`, `filename_patterns`) + `describe_expectation`.
- `rules`: engine `spec-pair` sem `require_non_empty_spec`, com ignores baked-in
  (`RULES.md:88-92`) e `ignore_files` como glob (D11) + `describe_expectation`.
- `cli`: `check`, `--format text|json`, `explain <path>`.
- Tier 2: primeiros snapshots `insta` de `check --format json`.

**Pronto quando:** roda contra um fixture que reproduz as regras atuais do
Flowmaatik e produz os mesmos findings que o script TS.

**Nota:** `Finding` carrega `Observed` + `Expectation` estruturados desde já —
`explain` nasce aqui, não no fim, senão o `explain` melhorado de v1
(`ROADMAP.md:71-73`) vira refactor.

**Registro** — 2026-07-25 11:45 → 14:20 -03 · `⚠️ concluído com desvio`

`archwarden check` roda contra um repositório de verdade. **342 testes**,
workspace em 98,2%, core em 100% de linha e função. Todos os 11 checks do CI
em 0.

**1. O trait `RuleEngine` estava errado e foi alargado.** Era orientado a
arquivo; `structure` e `spec-pair` perguntam sobre diretório. Ganhou
`check_directory` e `check_file`, ambos com default vazio. Refactor guiado por
uso real — o M1a desenhou o trait sem nenhuma engine para provar o desenho.

**2. `Finding` perdeu `#[non_exhaustive]`.** O atributo bloqueia *construção*
de outro crate, e toda engine constrói findings. Fica nos enums, que outros
crates só casam.

**3. `FileClass` mudou de crate.** Nasceu no `archwarden-engine`, que depende
de `rules` — a seta apontava errado quando o `spec-pair` precisou dele.
Classificação deriva só do path; foi para o `core`.

**4. `spec_suffix` virou `spec_markers`, por revisão do Henrique.** O modelo de
sufixo literal único não expressa o que vitest e jest aceitam
(`{test,spec}` × extensões). Pior: a primeira implementação tinha um bug
**codificado no próprio teste** — `Component.tsx` pedindo `Component.spec.ts`,
com a asserção afirmando isso. O vermelho nunca veio, e o `cargo-mutants`
também não pegaria: um teste que codifica a expectativa errada é consistente
consigo mesmo. Só apareceu quando alguém que usa o formato olhou.

**5. Todos os `Display` ignoravam largura de formatter.** `f.write_str()` não
honra `{:<7}`, então o alinhamento do relatório não funcionava — em silêncio.
Trocados por `f.pad()` nos sete tipos, com teste no `Level`.

**6. `warn_subfolders` precisou de uma observação própria.** Saía como
`warning` com a frase "is not allowed here", o que se contradiz. Virou
`Observed::DiscouragedSubfolder`, que diz "allowed for now, as documented
debt".

**7. Não usei `insta`.** O plano previa snapshots do `check --format json`.
Em vez disso: asserção campo a campo do envelope JSON, e um `assert_eq!` com
o texto escrito à mão para o formato de terminal. Motivo: para um contrato,
afirmar os campos declara o contrato, enquanto um snapshot só registra o que
saiu — e o `insta` foi mantido fora porque sua afordância principal
(`cargo insta review`) é exatamente o que inverte o ciclo TDD. Se você
preferir snapshots do documento inteiro para pegar campo novo aparecendo, é
adicionar depois; a dependência foi removida por ora.

**8. Não hasheio no walk**, apesar de o plano dizer que sim. Hashear exige ler
todo arquivo, e numa run só-estrutural são 30k leituras para nada. O M4 decide
a estratégia — provavelmente pré-filtro por mtime e tamanho antes de qualquer
leitura.

**Correção de doc pendente:** `RULES.md` listava `DOC.md` e `README.md` como
isenções embutidas do `spec-pair`. O filtro por `FileClass::Source` os cobre
junto com `.json`, imagens e tudo mais, então as entradas nominais viraram
letra morta. Já corrigido na reescrita da seção.

---

### M3 — Parser + `naming` `⚠️`

**Objetivo:** substituir o `lint-naming.ts`.

**Tarefas**
- `parser`: trait `Parser` + `OxcParser` (pin exato em `oxc_* 0.141.x`).
- `parser`: extração de `ExportFact` com as tags de D9.
- `rules`: engine `naming` (`file_pattern` com capture group, `must_export`,
  templating) + `describe_expectation` + `signature_hint` (D8).
- `rules`: `require_non_empty_spec` (`it`/`test`, sem `describe` — C3).
- Tier 1 cobrindo cada linha da tabela de D9.

**Pronto quando:** a tabela de D9 inteira tem teste; `naming` reproduz o
comportamento do script TS.

**Registro** — 2026-07-25 14:25 → 16:10 -03 · `⚠️ concluído com desvio`

`archwarden check` agora lê código. **381 testes**, workspace em 98%, core em
100% de linha e função. Todos os 11 checks do CI em 0.

**1. Explorei a API do `oxc` antes de escrever**, e isso mudou o desenho. Ela
tem **duas** fontes: o `module_record` sabe semântica de módulo (o que é
exportado, sob que nome, é default, veio de onde) e o AST sabe forma de
declaração. Nenhuma sozinha resolve o D9. Uso as duas e junto pelo nome do
binding local — que é o que faz `export { Local }` funcionar, com as duas
informações em statements diferentes. Escrever só o walker teria me obrigado a
reimplementar renomes, defaults e re-exports que o `oxc` já acerta.

**2. Três coisas que a exploração corrigiu antes de virarem bug:**
`logger.audit.write` saía como `?.write` (cadeia aninhada, e o `RULES.md` cita
esse formato como `must_call.symbol` válido); `.tsx` é gramática diferente e
JSX num `.ts` faz o parser desistir corretamente; e `panicked` é a falha dura,
não `diagnostics` — medi que decorators, generics e `declare module` produzem
zero diagnostics, então recusar por diagnostic recuperável estaria recusando
código que o `tsc` aceita.

**3. O trait ganhou `needs_facts()`.** Sem ele o runner teria que parsear todo
arquivo-fonte quando qualquer regra precisa de fatos. Com ele a decisão é por
arquivo: só lê o que uma regra que se aplica àquele arquivo realmente abre.
Numa config só-estrutural, nenhum byte é lido.

**4. Dois bugs que só apareceram rodando o binário**, nenhum teste unitário
teria pego:

- **`signature_hint` não era renderizado.** Saía `{{pascal(name)}}` literal no
  finding. Nunca é verificado, mas é *mostrado* — e mostrar o template ao
  usuário é mostrar nossas entranhas.
- **O spec vazio não era detectado.** O runner só oferece um arquivo à regra
  que diz aplicar-se a ele, e o `applies_to` do `spec-pair` devolvia `false`
  para specs (eles são isentos de precisar de spec próprio). Com
  `require_non_empty_spec` a regra *tem* o que dizer sobre o spec, então
  `applies_to` precisou distinguir os dois casos. Ambos com teste de regressão.

**5. Novos campos no `Report`: `unreadable_files`.** Um arquivo que não parseia
não foi checado, e um relatório limpo estaria mentindo. Reportado nos dois
formatos, junto com `unimplemented_rules`.

**Artefato local, não risco de CI:** intercalar `cargo llvm-cov` com builds
não-instrumentados na mesma sessão faz o piso falhar esporadicamente. Testei a
sequência exata do job de cobertura (só invocações de `llvm-cov`, nas duas
ordens) e passa. O job do CI é isolado, então não afeta o build.

---

### M4 — Cache `✅`

**Objetivo:** bater o critério warm do `ROADMAP.md:48-51`.

**Medição feita antes de começar (2026-07-25), e ela questiona a premissa do
ADR#3.** Com o M3 pronto e build em release, num repo sintético:

| | |
|---|---|
| 30.635 arquivos, 120 MB | **0,20 s** |
| 4.020 arquivos | 0,025 s |
| Pico de memória | 30 MB |

Sem cache algum. O orçamento do `ROADMAP.md` era 30k em menos de 5s a frio;
está 25× abaixo. O ADR#3 justifica o cache dizendo que "ferramentas
não-incrementais cruzam uma linha de usabilidade entre 10k e 100k arquivos" —
a 0,2s essa linha não está sendo cruzada.

Ressalvas: arquivos sintéticos são mais simples que TS real, page cache quente,
VM de 4 cores ARM, e cerca de metade dos arquivos foi parseada.

O cache **continua certo** para o watch mode de v1 e para o hook de pré-escrita
(onde 20ms importa), mas como bloqueador do v0 perdeu a urgência. Henrique
optou por manter a ordem do plano em 2026-07-25; registrado aqui para que a
decisão seja informada e não implícita.

**Tarefas**
- ✅ `cache`: store `redb`, duas tabelas — `facts[content_hash]` e
  `findings[content_hash + rules_hash + resolution_epoch]` (C5).

  **Formato de valor mudou de `postcard` para MessagePack (`rmp-serde`).** Um
  `Finding` carrega `Observed` e `Expectation`, que usam
  `#[serde(tag = "type")]` porque o relatório JSON é contrato com agentes. O
  serde **não consegue** serializar enum com tag interna num formato que não
  seja auto-descritivo — o que elimina `postcard` e `bincode` de uma vez. Os
  `facts` funcionavam com postcard; os `findings` não, e o teste de round-trip
  foi o que revelou. MessagePack é auto-descritivo e continua compacto.
- ✅ `engine`: `resolution_epoch` = hash de `tsconfig*.json` + `package.json` +
  lockfile (C4). Ficou em `archwarden-engine/src/epoch.rs`, não no `cache`:
  quem sabe quais arquivos existem é o `RepoTree`, e o `cache` não conhece
  árvore nenhuma. 11 testes.
- ✅ `cache`: versionamento de formato (ADR#3), invalidação total em bump.
- ✅ `engine`: probe antes de parse, flush em lote no fim. `check` passou a
  receber um `Run { root, config, tree, cache }` — struct e não quatro
  parâmetros, porque `Option<&mut Cache>` numa lista de argumentos não diz
  para que serve.
- ✅ `cli`: `--no-cache`, cache em `.archwarden/cache/cache.redb`, `files_parsed`
  e `facts_reused` no resumo (texto e JSON).
- ✅ `benches/`: criterion `walk` / `check/cold` / `check/warm` em 1k e 10k
  arquivos.

**Pronto quando:** warm run mede-se em `criterion` e a invalidação por mudança
de `tsconfig.paths` tem teste.

**Registro** — 2026-07-25

Tudo entregue, e a medição diz uma coisa desconfortável.

**Baseline (D13).** Lima Ubuntu 24.04, ARM64, 4 cores, build `--release`,
page cache quente, repo sintético de arquivos TS uniformes (~500 B, um
`import`, uma `interface`, uma `class`, a função exportada):

| | 1.000 arquivos | 10.000 arquivos |
|---|---|---|
| `walk` | 0,77 ms | 6,1 ms |
| `check/cold` (sem cache) | 5,53 ms | 61,6 ms |
| `check/warm` (cache cheio) | 4,60 ms | 55,4 ms |

**O cache economiza 10%.** Não 5×, não 2×: 10%. Fui atrás do porquê em vez de
registrar o número e seguir, e a decomposição para 10k arquivos é:

| Etapa | Custo | Observação |
|---|---|---|
| ler + hashear (`blake3`) | 22,8 ms | 2,28 µs/arquivo — inevitável |
| parse (`oxc`) | ~14 ms | **é isto que o cache poupa** |
| probe no `redb` | 8,0 ms | **é isto que o cache custa** |
| regras + contabilidade | ~25 ms | |

O parse do `oxc` custa 1,4 µs por arquivo. Ler o arquivo do disco custa 2,3 µs.
**Parsear é mais barato que ler.** O cache troca 14 ms de parse por 8 ms de
probe, e o ganho líquido some no ruído.

Dos 800 ns do probe: 214 ns são `begin_read` + `open_table` por arquivo,
164 ns a busca na árvore B, e ~390 ns o decode MessagePack. Reusar uma
transação de leitura ao longo do run salvaria ~2 ms em 55 ms — não vale a
complexidade de invalidar o snapshot depois do `flush`. Registrado como opção,
não feito.

**O que isto significa.** A premissa do ADR#3 já tinha sido questionada pela
medição de 2026-07-25 (30k arquivos em 0,20 s a frio). Agora há um segundo
dado: mesmo quando o cache funciona perfeitamente, ele não tem o que economizar,
porque a metade cara é o I/O e não o parse. As saídas reais, se um dia o
número importar:

- **(a) `mtime` + tamanho no lugar do hash de conteúdo** — troca o `read` por
  um `stat`, que é ~10× mais barato. É o que ferramentas de build fazem. Custa
  correção: um arquivo restaurado com o mesmo `mtime` e tamanho passa batido.
- **(b) cachear `findings` por diretório**, não `facts` por arquivo — pula
  também as regras (~25 ms), mas continua precisando saber que nada mudou.
- **(c) paralelizar com `rayon`** — ortogonal, 4 cores parados hoje, e
  provavelmente o maior ganho isolado.

Nenhuma foi feita. (a) é decisão de produto (correção por velocidade), (b)
depende do grafo do M5, (c) é M8. A tabela `findings` e o `resolution_epoch`
estão prontos e testados, mas **não estão ligados no runner** — ligar findings
por arquivo seria errado, porque a chave precisa cobrir a forma do diretório e
não o conteúdo de um arquivo. Fica para o M5, junto com o grafo.

**Desvios do esperado**

- `run::check` mudou de assinatura para o struct `Run`. Quarto caso em que a
  primeira implementação real de uma camada corrigiu a abstração desenhada
  antes dela.
- `reads_files(&CompiledConfig)` surgiu por causa de um teste: uma config só
  estrutural não deve **criar** o arquivo de cache. Abrir um `redb` que
  ninguém vai consultar deixa um arquivo que o usuário tem que descobrir por
  que existe.
- O JSON do resumo ganhou `files_parsed` e `facts_reused`. Não é bump de
  `REPORT_VERSION`: acrescentar campo não quebra consumidor.
- Texto só mostra `· N parsed, M reused` quando algo foi lido. Um run
  estrutural imprimindo `0 parsed, 0 reused` só levanta a pergunta.
- Dois furos de cobertura que **antecediam o M4** apareceram na conferência e
  foram fechados: `unreadable_files` não tinha teste nenhum (arquivo em
  Latin-1), e o marcador `<unreadable>` do epoch também não (`chmod 000`,
  `#[cfg(unix)]`).
- Erro meu, pego pelo compilador e não por revisão: escrevi o JSON de teste em
  `camelCase` (`filePattern`, `mustExport`). O formato documentado em
  `CONFIG.md:148` é `snake_case`.

**Um mutante sobrevivente, deixado de propósito.** `cargo mutants` no `engine`
(29 mutantes): 22 mortos, 6 inviáveis, 1 vivo — trocar `&&` por `||` em
`run.rs:152`, na guarda `file.class == FileClass::Source && algum engine
needs_facts()`. Ele sobrevive porque **toda engine de hoje já recusa arquivo
não-fonte por conta própria** (`spec_pair::is_exempt` checa `FileClass`,
`naming::applies_to` exige que o `file_pattern` case). Não existe entrada
alcançável em que as duas metades discordem.

Tentei matá-lo com um `.json` numa pasta governada por `spec-pair`; o teste
ficou (é contrato de verdade: aquele arquivo nunca é parseado) mas não mata o
mutante, porque `applies_to` já devolve `false` antes. Matar de fato exigiria
injetar uma engine de teste no `check`, o que significa mudar a assinatura para
receber engines só por causa da métrica.

A guarda **fica**. `FileFacts` vem de um parser de TypeScript; o invariante
"só arquivo-fonte vai para o parser" pertence ao único lugar que chama o
parser, não espalhado por toda regra futura — e D10 já prevê `.md` na árvore.
O crate `engine` não está no conjunto de zero-sobreviventes do plano
(`rules`, `config`, `core` estão).

---

### M5 — Resolver + `import-boundary` `✅`

**Objetivo:** grafo de imports próprio (ADR#7). Dividido em quatro por
tamanho, como o M1.

**Depende de:** resolução de C2 (shape do `import-boundary` no config) — feito.

---

#### M5a — Resolver `✅`

**Tarefas**
- ✅ `resolver`: `ImportResolver` TS-aware sobre o `oxc_resolver` —
  `tsconfig.paths`, extensões, `extension_alias`, condicionais de `exports`,
  workspaces, builtins.
- ✅ `resolver`: `InMemoryResolver` para fixture (`ARCHITECTURE.md:133`).

**Registro** — 2026-07-25

Duas descobertas de API, ambas achadas por teste vermelho e não por leitura:

1. **`resolve()` não faz descoberta automática de `tsconfig`.** Os dois testes
   de alias falharam com `NotFound` enquanto os outros onze passavam. O
   `oxc_resolver` documenta que `TsconfigDiscovery::Auto` **só funciona com
   `resolve_file()`** — a que recebe o caminho do *arquivo* importador, não do
   diretório. Faz sentido: o `tsconfig` que vale é o mais próximo acima do
   importador, e num monorepo isso é um arquivo diferente por pacote. De
   quebra o código ficou menor (some o cálculo do diretório pai).
2. **`import 'fs'` e `import 'node:fs'` normalizam para o mesmo nome.** O teste
   esperava `builtin fs` para a forma nua e veio `builtin node:fs`. É melhor
   assim — uma regra que proíbe `node:fs` pega as duas formas sem dizer duas
   vezes — mas era suposição minha e virou asserção explícita.

**Classificação.** `Resolved::InRepo` exige estar sob a raiz **e** não ter
`node_modules` em nenhum componente. Como o `oxc_resolver` segue symlink antes,
um pacote de workspace linkado em `node_modules/@org/domain` classifica pelo
lugar onde ele realmente mora — `packages/domain/src/index.ts`. É exatamente o
que uma regra de boundary escrita contra `packages/domain/**` precisa, e tem
teste (`#[cfg(unix)]`).

**Ordem das extensões e dos campos** é decisão, não acaso: `.ts` antes de `.js`
(num repo que tem os dois, a fonte é o arquivo sobre o qual a regra fala), e
`types` antes de `module` antes de `main` (o que um arquivo TS importa de uma
dependência são as declarações).

**Dívida da convenção, paga.** O `preset.rs` do M1 tinha dois
`let ... else { panic!() }` — a exata armadilha que virou convenção depois
dele. Trocados por asserção da mensagem inteira, que de quebra fixa a frase que
o usuário lê. O helper de teste `in_repo` que eu tinha acabado de escrever caiu
na mesma armadilha; virou `landed()`, que devolve `"in-repo src/x.ts"` /
`"external ..."` / `"builtin ..."` / `"error ..."`. A classificação passou a ser
metade da asserção em vez de ficar implícita num `matches!`.

Também troquei `if let Some(parent) = path.parent()` por `expect` nos helpers
de árvore temporária: o braço negativo nunca executa. O mesmo padrão está em
mais quatro crates e vale limpar quando eu passar por eles.

**Cobertura:** 98,42% linha no crate. Os 12 restantes são os dois braços
`NonUtf8Path` (`imports.rs` e `preset.rs`) — inalcançáveis porque a raiz é
`Utf8PathBuf` e o specifier é `&str`, mas exigidos pelo tipo de retorno do
`oxc_resolver`. `cargo mutants` nos dois arquivos novos: 3 mortos, 3 inviáveis,
**zero sobreviventes**.

**Fui rápido demais e escrevi a implementação antes do teste.** Percebi antes
de rodar qualquer coisa, salvei o esboço fora da árvore, reduzi o módulo a
stub que compila e falha, escrevi os 13 testes, vi os 13 vermelhos pelos
motivos certos, e só então restaurei. O ciclo foi cumprido, mas registro
porque a tentação de pular o vermelho quando o desenho já está na cabeça é o
modo de falha mais provável deste projeto.

---

#### M5b — Passe de resolução `✅`

**Tarefas**
- ✅ `engine`: `resolve_imports` preenchendo `ImportFact.resolved`, com tally
  por tipo de destino.
- ✅ `core`: `RuleEngine::needs_resolution()`, separado de `needs_facts()`.
- ❌ `engine`: índice reverso — **não construído, de propósito.** Ver abaixo.

**Registro** — 2026-07-25

**O índice reverso não tem consumidor no v0.** O `ARCHITECTURE.md:195` pede
"se o arquivo A mudou, quem importa A?" — e a razão declarada ali é
invalidação incremental de cache. O v0 não tem watch mode e re-checa o repo
inteiro todo run. Fui atrás de quem perguntaria, regra por regra:

| Regra | Pergunta que faz |
|---|---|
| `structure` | forma do diretório |
| `naming` | exports do próprio arquivo |
| `spec-pair` | irmãos do próprio arquivo |
| `import-boundary` | **os próprios imports** (`RULES.md:271-277`) |
| `call-obligation` | os próprios imports e chamadas |

Nenhuma pergunta "quem me importa". Construir o índice agora seria código sem
um requisito contra o qual testá-lo. Fica para o v1, junto com o watch mode que
é o motivo dele existir. Registrado aqui e no `//!` do módulo para que a
ausência seja decisão e não esquecimento.

**Destino do import: contado, não guardado.** O `ImportFact.resolved` é
`Option<RepoRelPath>`, e as globs de uma boundary rule são caminhos relativos
ao repo (`RULES.md:281-285`). Uma dependência instalada e um builtin não têm
caminho que uma glob dessas possa casar — mas se os dois virassem `None` junto
com "não resolveu", o relatório não conseguiria distinguir *"lodash é uma
dependência"* de *"lodash não foi encontrado"*. Num repo sem `node_modules`
instalado isso é a diferença entre silêncio e três mil ruídos.

Solução: só `InRepo` chega ao fato; `External`, `Builtin` e falha são
**contados** num `Outcomes`. O tipo do fato não precisa aprender sobre
dependências, e o relatório consegue dizer a verdade.

**Limitação que isso deixa, para o Henrique decidir depois:** no v0 não dá para
escrever *"a UI não pode importar `lodash` direto"* nem *"ninguém importa
`node:fs`"*. As globs só alcançam caminhos do repo. O `RULES.md` não pede isso
— todos os exemplos são camadas internas — mas é uma regra de arquitetura
plausível, e agora existe um `Outcomes.builtin`/`.external` de onde partir.

**`needs_resolution()` é separado de `needs_facts()`** porque resolver é um
segundo custo em cima de parsear: bate no filesystem para cada specifier de
cada arquivo. Uma regra de naming lê dentro do arquivo e nunca pergunta para
onde os imports vão — não deve pagar por isso.

**Escopo movido para o M5c.** A ligação do passe no runner ficou de fora
deliberadamente: nenhuma engine do v0 devolve `needs_resolution() == true`
ainda, então o encanamento não teria como ser testado através do `check()`.
Vai junto com a engine que o liga, que é onde ele passa a ter consumidor.

`cargo mutants` no `resolve.rs`: **26 mutantes, 26 mortos.**

---

#### M5c — `import-boundary` `✅`

**Tarefas**
- ✅ `rules`: `ImportBoundaryEngine` — `forbid_import_from`,
  `must_import_from`, `except`, `include_type_only` + `describe_expectation`.
- ✅ `engine`: `resolve_imports` ligado no runner, `ImportResolver` construído
  uma vez por run e só quando alguma engine pede, `Outcomes` no `Report`.
- ✅ `cli`: nota de imports não resolvidos, resumo `imports` no JSON.

**Registro** — 2026-07-25

Primeira regra que é sobre uma *relação* e não sobre um arquivo. Tudo antes
dela se decide com o nome e o conteúdo de um arquivo só.

**Dois desvios, ambos apontados por teste.**

1. **Meus dados de teste estavam errados, não o código.** Oito de dezesseis
   testes falharam depois de restaurar a implementação. A causa era o escopo
   que eu escrevi: `packages/ui/*` seleciona *diretórios um nível abaixo* de
   `packages/ui` (D4), então `packages/ui/a.ts` — cujo pai é `packages/ui` —
   ficava fora. O único teste que passava usava
   `packages/ui/button/button.tsx`. Um boundary é sobre um pacote inteiro, e a
   grafia certa é `packages/ui/**`. Anotado no helper para o próximo não cair.
2. **A suíte pegou um teste que virou mentira.** `a_rule_with_no_engine_is_
   named_in_the_report` usava `import-boundary` como exemplo de "kind sem
   engine" — o que deixou de ser verdade no instante em que registrei a
   engine. Trocado para `call-obligation`, que é o último sem engine. Um teste
   que codifica "ainda não implementado" tem prazo de validade, e é bom que
   ele falhe alto quando vence.

**Decisões de desenho**

- **`except` só protege contra `forbid`.** Uma exceção a um *requisito* seria
  um requisito que ninguém precisa cumprir. Documentado no `RULES.md`.
- **A resolução roda depois do cache, nunca antes.** O que é guardado é a
  saída do parser, com chave só de conteúdo. Resolver depende de arquivos que
  nenhum hash de conteúdo cobre (`tsconfig`, lockfile), então cachear fato
  resolvido exigiria o `resolution_epoch` na chave e serviria caminho velho no
  dia em que alguém mexesse num alias. Verificado no binário: a segunda rodada
  reusa os fatos do cache e resolve tudo de novo, com o mesmo resultado.
- **O `ImportResolver` é construído uma vez por run**, não por arquivo — o
  `oxc_resolver` cacheia leituras de `tsconfig` e `package.json` internamente,
  e um resolver novo por arquivo jogaria isso fora milhares de vezes.
- **O finding carrega o span do `import`.** Primeira regra em que o span é
  útil de verdade: aponta para a linha exata que o usuário tem que apagar.
- **Specifier e caminho resolvido, os dois no finding.** Com alias eles não se
  parecem: o usuário precisa do specifier para achar a linha e do caminho para
  entender por que a regra disparou.

**Verificado no binário**, num repo com alias, `except`, builtin e dependência
faltando:

```
error   packages/ui/button.tsx
        [*] ui-forbids-domain — imports `@/domain/user/user.entity`,
            which resolves to `packages/domain/user/user.entity.ts`
        expected: no import from `packages/domain/**`,
                  except `packages/domain/*/types/**`

note: 1 import could not resolve, so boundary rules did not see it
1 error, 0 warnings · 5 files, 6 directories · 1 parsed, 0 reused
```

O `import type` do `types/` não disparou, o `node:fs` foi contado como builtin
e o `@org/never-installed` como não resolvido.

`cargo mutants` no `import_boundary.rs`: 34 mutantes, **24 mortos, 10
inviáveis, zero sobreviventes** — depois de matar um: o acessor `module()`
sobrevivia porque nenhum teste declarava a regra dentro de um módulo.

**`RULES.md` ganhou duas limitações explícitas:** `except` não vale para
`must_import_from`, e no v0 não dá para proibir uma dependência ou um builtin
pelo nome.

---

#### M5d — Tier 3 `✅`

**Tarefas**
- ✅ `crates/archwarden-engine/tests/differential.rs`, atrás da feature
  `differential`.
- ✅ `tests/differential/known-divergences.md`, lido pelo harness — o markdown
  é a fonte da verdade, não uma lista em código que ia divergir da
  justificativa ao lado.
- ✅ Job `differential` do nightly ligado (era `if: false`).

**Registro** — 2026-07-25

**O harness pegou um erro na primeira execução — no meu documento, não no
código.** Eu tinha escrito no `known-divergences.md` que "re-export não é aresta
de import em archwarden" e implementado o filtro correspondente. O parser faz o
contrário desde o M3, e de propósito: `oxc.rs:325` diz *"a re-export's source is
an import for the purpose of the graph"*. Um arquivo que faz
`export * from '@/domain'` depende de domain — depende mais, inclusive, porque
republica sob o próprio nome. Filtro removido, documento reescrito, e a entrada
ficou lá como registro de uma suposição que a documentação sustentou e o código
não.

**Divergência real encontrada, e archwarden está certo.** Subpath de `exports`
em pacote de workspace: `import { X } from '@org/domain/types'` onde
`@org/domain` é symlink para `packages/domain/user` e o `package.json` mapeia
`"./types"` para um arquivo. O archwarden segue o mapa e o symlink e chega na
fonte; o `dependency-cruiser` 18.1 resolve o `@org/domain` nu mas devolve
`couldNotResolve` para o subpath.

Isso levantou uma pergunta de desenho do harness: **`couldNotResolve` é
admissão de ignorância, não afirmação de ausência.** Comparar contra uma
admissão de ignorância produz ruído, não sinal. O harness passou a separar as
duas coisas: aresta que o `dependency-cruiser` *colocou* e o archwarden não viu
continua sendo falha; aresta que ele desistiu de colocar vira nota
`more than the reference`. O inverso — mesmo par caindo em arquivos diferentes
— continua falhando dos dois lados.

**Validado contra o Flowmaatik.** O repo não estava alcançável na primeira
tentativa — o `ai-jail` só expunha o `archwarden`. Henrique adicionou
`--map .../Flowmaatik:/mnt/flowmaatik` (read-only) e a corrida saiu:
**22 pacotes, 3.269 arquivos TS/TSX, ~6.300 arestas.**

| | |
|---|---|
| Pacotes que batem aresta por aresta | **20 de 22** |
| Divergências reais | **6 arestas**, uma única causa |

Rodado com o `dependency-cruiser` deles (17.4.3, em
`packages/application/node_modules/.bin`) e com o 18.1.0 do meu scratchpad —
números idênticos.

Antes disso o harness tinha sido validado contra um repo sintético com alias de
`tsconfig.paths`, alias de subpath, resolução por `index`, `.js` significando
`.ts`, dependência circular, `.d.ts`, import de efeito colateral, workspace por
symlink e `exports` com subpath. O fixture não é commitado — os alvos vão por
variável de ambiente, como o `TESTING.md` manda.

**O achado: `import()` dinâmico é invisível para o archwarden.** As seis
divergências são todas isto, em duas formas:

```ts
const { mapReaction } = await import("./mappers/map-reaction");   // expressão
actor: import("../../actor/actor").Actor;                          // posição de tipo
```

O parser lê `module_record.import_entries` e `requested_modules`, que cobrem só
sintaxe **estática** de módulo. Um `import()` é expressão de chamada e não
aparece. O `dependency-cruiser` pega as duas formas.

Pelos três casos do `TESTING.md` isto é o **(a): archwarden está errado, e se
corrige** — não vira entrada de divergência conhecida. Uma boundary rule hoje é
contornável escrevendo `await import('@/domain/user')`, e ninguém precisa
querer contornar: code-splitting é normal. No Flowmaatik são 208 `import()` em
46 arquivos, **7 deles apontando para outro pacote `@flowmaatik/*`** — que é
exatamente o que uma boundary rule existe para pegar. Virou o M5e.

**Erro meu na primeira passada, corrigido.** Apareceram 4 divergências extras em
`apps/app` que eram artefato de invocação: passei `ARCHWARDEN_DIFF_DIRS=src`
enquanto o archwarden caminha o pacote inteiro, então `scripts/` e `e2e/` só
existiam de um lado. Com os diretórios casando, `apps/app` fecha em 1588×1588.

**Confirmado em repo real o que o fixture já apontava:** o `packages/domain`
tem **93 arestas que só o archwarden vê**, todas `exports` com wildcard
(`"./address/*": "./src/address/*.ts"`), que o `dependency-cruiser` não resolve
em nenhuma das duas versões. Ali quem está certo somos nós, e o harness reporta
como nota.

**Descoberta de ferramenta que vale para quem for rodar:** o
`dependency-cruiser` 18 declara `typescript >=2 <7` e, com TypeScript 7
instalado, **cruza zero arquivos sem dizer por quê** — `totalCruised: 0` e
`transpilersFound[typescript].available: false` enterrado no resumo. Por isso o
harness afirma que o lado dele não veio vazio: uma comparação vazia passaria
pelo motivo errado. O job do nightly instala `typescript@5` explicitamente.

**Sem alvo configurado o teste diz por que não fez nada e passa.** Um teste
differential não tem como inventar um repositório contra o qual diferenciar, e
falhar por falta de um só ensinaria a ignorá-lo.

**Pronto quando:** Tier 3 roda contra o Flowmaatik sem divergência não
justificada — **rodou**, achou 6 divergências de uma causa só, e depois do M5e
fecha em **22 de 22 pacotes, zero divergências**.

---

#### M5e — `import()` dinâmico `✅`

Encontrado pelo M5d contra o Flowmaatik.

**Tarefas**
- ✅ `parser`: `import()` com specifier literal vira `ImportFact`, na forma de
  expressão e na de posição de tipo.
- ✅ Argumento não-literal fica de fora.
- ✅ Differential reconferido contra o Flowmaatik.

**Registro** — 2026-07-25

**22 de 22 pacotes, zero divergências não explicadas.** Antes eram 20 de 22 com
6 arestas divergentes; as seis eram esta.

O `module_record` do `oxc` cobre sintaxe de **módulo**. Um `import()` é
expressão de chamada, então sai pelo AST — um `Visit` que trata dois nós:

| Nó | Marcação |
|---|---|
| `ImportExpression` (`await import('./x')`) | `type_only: false` |
| `TSImportType` (`import('./x').T`) | **`type_only: true`** |

A segunda marcação é decisão, não detalhe: `import("./a").Actor` numa anotação
de tipo é apagado na compilação, então uma regra com `include_type_only: false`
não deve enxergá-lo. Virou asserção no teste.

**Specifier não-literal fica de fora.** `import(name)` e
`import(`./locales/${name}`)` não nomeiam um módulo, e inventar um faria uma
boundary rule reportar um caminho que ninguém escreveu. O tally `unresolved` do
run é onde o usuário fica sabendo que a corrida viu menos que tudo.

**`require()` continua de fora**, e tem teste dizendo isso. Resolução `CommonJS`
tem regras próprias que o v0 não promete seguir; pegar a string aqui prometeria
uma cobertura que o resolver não tem.

**Seis mutantes sobreviventes no `oxc.rs`, todos anteriores a este step.**
`cargo mutants`: 49 mutantes, 35 mortos, 8 inviáveis, 6 vivos — nenhum no
código novo (`dynamic_imports` / `DynamicImportCollector` ficaram limpos).
Os seis são:

- 4 em `declaration_tags` / `record_default` — as tags de
  `export default function` e `export default class`;
- 2 na chave de deduplicação do `imports` (`span == span && specifier ==
  specifier`), que com `||` juntaria dois `import` do mesmo módulo em
  statements diferentes.

O `archwarden-parser` **não está** no conjunto de zero-sobreviventes do plano
(`core`, `config`, `rules` estão), então isto está dentro da tolerância
acordada — mas é lacuna de teste real e fica registrada aqui em vez de sumir.

---

### M6 — `call-obligation` `✅`

**Objetivo:** a regra que nenhum outro tool faz.

**Tarefas**
- ✅ `parser`: `CallFact` com method chains — já vinha do M3, reaproveitado
  sem mudança.
- ✅ `rules`: `CallObligationEngine` — checagem de `imported_from`, falha
  específica "expected import missing", `describe_expectation`.

**Pronto quando:** obrigação satisfeita via helper local é detectada;
cross-file continua fora de escopo, com mensagem clara. **Atendido.**

**Registro** — 2026-07-25

Primeira regra sobre **comportamento** e não sobre forma. Todo o resto checa
onde o arquivo está, como se chama, o que exporta, o que alcança. Esta checa se
ele *fez* uma coisa.

**Duas falhas, mantidas distintas de propósito.** "Você não importou
`Event.save`" e "você importou e nunca chamou" são erros diferentes com
correções diferentes. A checagem do import vem primeiro e a falha dela encerra
o exame: dizer que um arquivo nunca chama um símbolo que ele nunca importou
manda o leitor procurar um call site em vez de um import.

Verificado no binário:

```
error   .../route.delete.ts
        [*] non-get-routes-must-audit — `Event.save` is not imported from `@flowmaatik/domain/event`
error   .../route.put.ts
        [*] non-get-routes-must-audit — `Event.save` is imported but never called

2 errors, 0 warnings · 5 files, 7 directories · 3 parsed, 0 reused
```

O `route.post.ts`, que chama via helper local, passa. O `route.get.ts` não casa
o `file_pattern` e não é examinado.

**Decisões de desenho**

- **`imported_from` casa o specifier como escrito**, não o caminho resolvido.
  É o que a palavra diz — *de qual pacote* o símbolo vem — e é por isso que
  esta regra tem `needs_resolution() == false`, ao contrário da
  `import-boundary`. Custo: quem importar por caminho relativo em vez do nome
  do pacote não casa. Está documentado.
- **Import type-only não satisfaz.** Não se chama um tipo, e um
  `import type { Event }` satisfazendo a metade-import de uma regra cuja razão
  de existir é a chamada seria a pior espécie de falso negativo.
- **A raiz do símbolo é o que o import precisa dar.** `Event.save` é chamado
  através do binding `Event`; `saveEvent` é raiz de si mesmo.
- **Method chain casa exatamente.** `Event.saveDraft`, `save`, `Other.save` e
  `Event.save.later` não satisfazem `Event.save`.

**Correção C10 — `CONFIG.md:245` prometia mais do que o v0 faz.** O texto dizia
*"AST call-graph walk within the file (following local function definitions) to
check that at least one reachable path from the top-level export calls
`Event.save`"*. O que está implementado é **contenção plana**: a chamada em
qualquer lugar do arquivo satisfaz.

A diferença é só código morto — uma função que ninguém alcança e que chama o
símbolo. E o `RULES.md:170-172` já abria mão disso explicitamente ("calls
inside `if (false)` ... are not filtered out. archwarden is a structural
linter, not a taint analyser"). Os dois documentos se contradiziam; o
`CONFIG.md` foi corrigido para descrever o que existe, e a razão ficou escrita
ao lado. Contenção plana também é o que atende o critério do plano
(helper local detectado) sem máquina nenhuma de grafo.

**Um teste com prazo de validade venceu, de novo.** O
`a_rule_with_no_engine_is_named_in_the_report` do `run.rs` usava
`call-obligation` como exemplo de kind sem engine — segunda vez que isso
acontece (a primeira foi o `import-boundary` no M5c). Substituído por três
testes ponta a ponta da regra através do parser real.

**E isso levanta uma decisão que é sua.** Com o M6, **todos os cinco kinds do
v0 têm engine**, então o braço `else` do `engines_for` e o campo
`unimplemented_rules` do `Report` viraram inalcançáveis — código que nenhuma
execução atinge, que é exatamente o que a convenção deste projeto condena.

**Henrique escolheu (a) em 2026-07-25:** `engines_for` virou `match` exaustivo
em `CompiledRuleKind`, e "kind sem engine" deixou de ser estado possível.

**O que isso exigiu, e não era óbvio.** O `match` exaustivo não compilava:
`CompiledRuleKind` era `#[non_exhaustive]`, e essa marca obriga um braço
curinga em *qualquer outro crate* — que é exatamente o `else` que se queria
eliminar. A marca saiu, com a razão escrita ao lado do tipo: ela existe para
que um variante novo não quebre quem casa o enum de fora, e aqui **queremos**
que quebre. Os oito crates versionam em lockstep e não há downstream
independente, então ela não comprava nada e custava a garantia.

Isso é a mesma distinção já registrada no M2 — `#[non_exhaustive]` é certo para
enum que outros só **casam** (`Observed`, `Expectation`, que o `report.rs`
renderiza e precisa continuar compilando), e errado quando o casamento
exaustivo é o mecanismo. Anotado no próprio tipo para o próximo leitor.

Os construtores de todas as cinco engines ganharam um `build(...)` infalível
que recebe o payload já desestruturado; o `from_rule(&rule) -> Option<Self>`
continua como API pública e é o que os testes usam, incluindo o caso "kind
errado devolve `None`". Nada ficou inalcançável.

**Saiu do `Report` e do JSON:** `unimplemented_rules`.

**`REPORT_VERSION` não foi bumpado**, e a exceção está escrita no código: o
campo já era omitido de todo relatório limpo, só podia aparecer num estado que
nenhuma build publicada alcança, e o archwarden não foi publicado. A versão 0
continua sendo a primeira forma que qualquer consumidor vai ver. Remoção de
campo **depois** do release é bump.

O `unreadable_files` continua e ganhou teste próprio nos dois formatos: agora
é o único jeito de um run admitir que viu menos do que tudo.

---

### M7 — Superfície de agente `✅`

**Objetivo:** ADR#9 completo — informante, não só gate. Dividido em cinco por
tamanho, como o M1 e o M5.

**Pronto quando:** `describe`/`scaffold` respondem <50ms warm
(`ROADMAP.md:54`); hook do Claude Code bloqueia escrita inválida com mensagem
que identifica regra e correção (`ROADMAP.md:55-57`).

---

#### M7a — `describe` `✅`

**Tarefas**
- ✅ `cli`: `describe <path>` (text + JSON), sem parse, Tier 2 incluído.

**Registro** — 2026-07-25

**O seam desenhado no M2 pagou.** O `RuleEngine::describe_expectation` existe
desde o M2 justamente para isto, com o contrato de ser puramente lexical — e
foi por isso que este step virou quase só renderização. As cinco engines já
respondiam certo sobre um caminho que não existe; não precisei tocar em
nenhuma.

**"Aplica" quer dizer "tem exigência", não "a glob casou".** Uma regra de
`naming` cujo escopo cobre `src/user/` mas cujo `file_pattern` não casa
`helper.ts` **não** aparece na resposta. Um agente que recebesse a regra sem
exigência ficaria tentando satisfazer algo que nunca vai disparar.

**`ignore` ganha do escopo, e o `describe` concorda com o `check`.** Se
divergissem, o agente seria mandado satisfazer uma regra que nunca vai rodar —
pior que não responder.

**Resolução de caminho é lexical, nunca toca no disco.** É o ponto todo:
`describe` responde sobre arquivo que não existe, então `canonicalize` não está
disponível. O `RepoRelPath::new` já normalizava `.` e `..` desde o M2, então
sobrou traduzir "onde o usuário está" para "relativo à raiz". Coberto para
subdiretório, caminho absoluto, e recusa fora do repo com mensagem que nomeia
as duas pontas.

**Uma renderização só.** O `describe_expectation(&Expectation) -> String` do
`report.rs` virou `pub(crate)` e é o mesmo que o `check` usa. O informante e o
gate não conseguem redigir a mesma exigência de formas diferentes — que é o
que a ADR#9 pede.

**`DESCRIBE_VERSION` é separado do `REPORT_VERSION`.** Um agente que consome um
pode nunca ler o outro, e acoplar os dois forçaria bump em quem consome um
contrato que não mudou.

**Latência: 2 ms por chamada** (10 execuções, release, repo pequeno). O
`ROADMAP.md:54` pede <50 ms e o `AGENT-INTEGRATION.md:41` pede <20 ms. E o
custo é O(regras), não O(arquivos) — o `describe` não caminha a árvore —, então
o número não piora com o tamanho do repositório.

`cargo mutants` no `describe.rs`: 7 mutantes, 5 mortos, 2 inviáveis, **zero
sobreviventes**.

---

#### M7b — `scaffold` `✅`

**Tarefas**
- ✅ `cli`: `scaffold <path>` (text + JSON), Tier 2 incluído.

**Registro** — 2026-07-25

**É uma transposição, não uma segunda travessia.** O `scaffold` é construído em
cima do `describe`: duas varreduras das mesmas regras poderiam discordar, e aí
um agente que seguisse o `scaffold` reprovaria no `check`. O `describe` responde
regra a regra; quem vai escrever um arquivo não pensa regra a regra, pensa em
uma lista de exports, uma de irmãos, uma de restrições de import.

**Uma entrada por glob, não por regra.** Um `forbid_import_from` com três globs
vira três entradas, cada uma carregando o `except` da regra. O agente pergunta
"posso importar isto?" sobre um caminho de cada vez, e uma lista que ele tem
que desempacotar antes é uma lista que ele erra.

**Correção C11 — o shape do `AGENT-INTEGRATION.md:145` estava incompleto.**
Faltavam duas coisas:

- `filename_patterns` — sem isso, um agente montando um caminho cujo **nome** já
  está errado recebe tudo menos o que ele precisa consertar primeiro;
- `allowed_subfolders` — o `describe` já responde sobre diretório, e um
  `scaffold` que perdesse a resposta seria menos útil que o comando em que se
  apoia.

Também troquei `"kind": "function"` por `"kinds": ["function"]`: `kind:
["function","arrow"]` é jeito normal de dizer "chamável, qualquer forma", e a
lista vazia é como o `any` diz que não pede forma nenhuma. Documento
atualizado.

**Uma aresta afiada que fica registrada.** O `signature_hint` é reproduzido
literalmente depois da palavra-chave, então um hint em estilo arrow
(`(deps: Deps) => UseCase`) sob uma regra que exige `kind: "function"` produz:

```
export function CreateClient(deps: Deps) => UseCase<In, Out>
```

que não compila. Está **certo** pelo contrato — o `RULES.md` diz que o hint é
"never verified" e o exemplo do próprio doc usa o estilo com dois-pontos — mas é
o tipo de coisa que um usuário descobre tarde. **Não é para o `scaffold`
resolver:** validar hint contra kind é checagem semântica de config, que é
exatamente o `config doctor` do M8. Anotado lá.

**Latência: 2 ms**, mesma do `describe` e pelo mesmo motivo — nenhum arquivo é
lido.

---

#### M7c — `agent-guide` `✅`

**Tarefas**
- ✅ `cli`: `agent-guide --format markdown|json --scope <path>`,
  determinístico, Tier 2 incluído.

**Registro** — 2026-07-25

**Correção C12 — o mecanismo que o `ARCHITECTURE.md:252` descrevia não é
implementável.** O texto dizia que o `agent-guide` "itera cada regra da config e
chama o mesmo `describe_expectation()` por regra". Ele não pode: aquele método
recebe um **caminho**, e de propósito — a expectativa de uma regra `naming`
carrega o nome do export **já renderizado**, e o nome vem do nome do arquivo.
Um guia não tem nome de arquivo. Inventar um encheria o digest de nomes
derivados de um caminho que ninguém vai criar.

Verifiquei antes de desenhar, no `naming.rs`: `expectation(&self, path)` começa
com `path.file_name()?`. Não é detalhe de implementação, é a natureza da coisa.

**A propriedade que aquela seção queria sobrevive assim mesmo.** O guia
renderiza a `CompiledRule` — que é o mesmo valor que as engines consomem —,
então ele não consegue errar as globs, os patterns ou os templates de uma
regra. E as respostas precisas por caminho continuam sendo o `describe` e o
`scaffold`, que passam pelo seam das expectativas. Documento corrigido com o
raciocínio junto.

**Determinismo é requisito, não consequência.** O `AGENT-INTEGRATION.md:184`
diz que a saída pode ser commitada ou regenerada sob demanda. Se um dos dois
gerasse bytes diferentes, quem escolheu o outro veria diff que ninguém fez. Por
isso não há timestamp, versão nem nome de máquina na saída — e tem teste de
duas execuções byte a byte, mais um assertando que nenhum "202" aparece.

**`--scope` olha nas duas direções.** Quem pede "o guia de `packages/domain`"
quer tanto a regra com escopo `packages/**` (que governa aquele diretório)
quanto a com escopo `packages/domain/src/*` (que vive dentro dele). Uma direção
só deixaria metade das regras de fora, e a metade que falta muda conforme como
o usuário escreveu a config — pior que não filtrar.

**Sai por stdout, não para arquivo.** O próprio doc mostra
`agent-guide > .archwarden/AGENT_RULES.md`; um comando que escolhesse o destino
sozinho escreveria onde o usuário não pediu, e o `AGENT-INTEGRATION.md:229`
lista isso como não-objetivo explícito.

`cargo mutants`: 26 mutantes, 4 sobreviventes na primeira rodada — o braço da
raiz no `--scope`, e as três metades de uma regra `structure` que constrange só
uma coisa (só `warn`, só filenames, só subfolders). Três testes fecharam os
quatro. **Zero sobreviventes.**

---

#### M7d — `check --file` `✅`

**Tarefas**
- ✅ `engine`: `single::check_file`, uma leitura por diretório do caminho.
- ✅ `cli`: `check --file <path>` com `skipped` explícito (C6), Tier 2 incluído.

**Registro** — 2026-07-25

**Correção C13 — o "cold-cache" do `AGENT-INTEGRATION.md:180` não existe.** O
texto dizia que as graph rules precisam de estado cross-file, só disponível com
cache quente, e que rodar o grafo a cada escrita estouraria o orçamento de
latência. As duas metades estão erradas:

1. Uma boundary rule é **file-local depois da resolução** — ela pergunta sobre
   os *próprios* imports. Isso já estava estabelecido no M5b, quando decidi não
   construir o índice reverso por falta de consumidor.
2. Resolver custa pouco. **Medido: 3 ms por invocação** contra o `node_modules`
   e o `tsconfig` reais do Flowmaatik, num arquivo com 4 imports, incluindo a
   checagem de diretório do `spec-pair`.

Verificado que a regra **dispara mesmo**, não passa em silêncio: com um
`import '../internal/secret'` plantado, o `check --file` devolveu
`forbidden-import` com o caminho resolvido, e `skipped: []`.

**O desenho mudou no meio, e para melhor.** Comecei assumindo que regras de
diretório seriam puladas e reportadas. Aí descobri, lendo o `spec_pair.rs`, que
a checagem de irmão ausente vive no `check_directory` — ou seja, a falha que um
hook pré-escrita mais precisa pegar ("seu arquivo novo não tem spec") ficaria
de fora. E a de `structure` também ("você criou uma pasta proibida").

Um hook que perde duas das cinco regras é um hook fraco. Então o comando roda
as regras de diretório também, com **uma listagem por ancestral e a escrita
dobrada dentro** — nem o arquivo nem as pastas até ele existem quando o hook
pergunta, e checar a árvore como ela está perderia exatamente o que o hook
existe para pegar. Os findings são filtrados para a ancestralidade do próprio
caminho: quem está escrevendo um arquivo não recebe o problema do vizinho.

**`skipped` sobrou com dois motivos, ambos reais:** `unreadable` (o arquivo não
foi lido ou parseado) e `not-source` — este último apareceu por causa de um
mutante. Uma `call-obligation` com `file_pattern` casando `^data\.json$` era
reportada como "não deu para ler", quando o arquivo está perfeito e quem está
errado é a regra. Motivos opostos exigem correções opostas, então viraram
slugs distintos.

**Um mutante sobrevive, e é o mesmo padrão do M4.** A guarda `is_source` antes
de parsear: o parser recusa extensão não-fonte de qualquer jeito, então trocar
`&&` por `||` chega na mesma resposta por um caminho com uma leitura
desperdiçada. Fica porque ler um arquivo para descobrir que ele é do tipo
errado é trabalho sem motivo — e num binário que casou um `file_pattern` é
trabalho grande. Segunda ocorrência da forma; anotada no código.

---

#### M7d.1 — campos desconhecidos na config `✅`

Bug encontrado por acidente durante o M7d: escrevi `"allow"` num teste onde o
campo é `"allowed_subfolders"`, e nada reclamou.

**Registro** — 2026-07-25

O `config validate` dizia *"is valid (1 rule)"* e o `check` reportava
*"0 errors"* — com uma regra `structure` que não constrangia coisa nenhuma.
**Uma regra que silenciosamente não enforça nada é a pior falha possível num
linter, porque é indistinguível de uma regra que passa.** É a mesma classe do
C6, um nível acima: lá era regra pulada em silêncio, aqui é regra desarmada em
silêncio.

`#[serde(deny_unknown_fields)]` nos dez tipos de wire — `Config`, `Module`,
`SkipDirs`, as cinco regras, `MustExport` e `MustCall`. O diagnóstico que sai
já era bom de graça, porque o `serde_path_to_error` do M1 dá o caminho e o
serde lista as alternativas:

```
× ... at `rules[0]`: unknown field `allow`, expected one of `id`, `level`,
  `roots`, `allowed_subfolders`, `warn_subfolders`, `recurse_into`,
  `filename_patterns`
```

**Efeito colateral bom no schema.** O `schemars` traduz o atributo para
`additionalProperties: false` — 10 lugares —, então um editor com `$schema`
ligado pega o erro **antes** de rodar qualquer coisa. Validei com um validador
JSON Schema de verdade: config correta `VALID`, config com `allow` `INVALID`.

O `$defs` do schema caiu de 15 para 10 porque o `schemars` passou a **inlinar**
os cinco variants em vez de referenciá-los — com tag interna, o `type` precisa
entrar nas `properties` de cada um para o `additionalProperties: false` não
rejeitar tudo. Conferi que entrou: `required: [type, id, level, roots]`.

**O custo, e é decisão consciente:** uma config escrita para um archwarden mais
novo passa a ser **recusada** por um mais velho, em vez de degradar. É a troca
certa — o arquivo é pequeno, tem campo `version`, e um palpite errado sobre o
que uma chave significa é pior que um erro. Documentado no `CONFIG.md`.

---

#### M7e — `install-hooks` e `init` `✅`

**Tarefas**
- ✅ `cli`: `hook claude-code` — lê o evento do stdin e responde no protocolo.
- ✅ `cli`: `install-hooks --claude-code [--remove]`, idempotente.
- ✅ `cli`: `init`.
- ✅ Tier 2 do setup recomendado inteiro, ponta a ponta.

**Registro** — 2026-07-25

**Correção C15 — o hook não recebe `$CLAUDE_FILE_PATH`.** O
`AGENT-INTEGRATION.md:168` dizia que o comando instalado seria
`archwarden check --file $CLAUDE_FILE_PATH`. Fui conferir num hook que funciona
de verdade (o `pre-tool-use.sh` do ai-memory, instalado nesta máquina): o
Claude Code entrega o evento como **JSON no stdin**, e o alvo da escrita está
em `tool_input.file_path`.

Então o comando instalado é `archwarden hook claude-code`, que lê o payload
sozinho — um binário, sem aspas de shell, e sem depender de um `jq` que o
usuário pode não ter.

**O protocolo de resposta veio de um plugin oficial, não de palpite.** O
`hookify` do marketplace oficial emite, para negar:

```json
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny"},
 "systemMessage":"..."}
```

Emito exatamente isso. Procurei `permissionDecisionReason` na máquina inteira e
não achei uso, então não inventei o campo — a explicação vai no
`systemMessage`, que é onde o plugin oficial põe.

**O hook nunca bloqueia por falhar.** Payload ilegível, ferramenta que não
escreve arquivo, config quebrada, config de versão futura — cada caso libera a
escrita. Bloquear é decisão carregada na resposta, nunca efeito colateral de
algo dar errado. Tem teste para os quatro casos. É o que o `pretooluse.py`
oficial também faz, com um comentário em caixa alta: *"ALWAYS exit 0 — never
block operations due to hook errors"*.

Finding de warning aparece sem bloquear, pela D1.

**Editar o `settings.json` do usuário exigiu uma mudança de dependência.** O
`serde_json` usa `BTreeMap` por padrão, então um round-trip **alfabetiza todas
as chaves** — um diff enorme num arquivo que é do usuário e que ele não pediu
para reformatar. Confirmei com um probe antes de escrever qualquer coisa:
`{"z":1,"a":2,"m":3}` volta como `{"a":2,"m":3,"z":1}`. A feature
`preserve_order` entrou no workspace por causa disso, e tem teste assertando
que a ordem das chaves do usuário sobrevive.

**Idempotência de verdade, não só "não duplica".** A entrada é reconhecida pelo
**comando**, não pelo bloco inteiro, então quem estreitou o matcher ou pôs um
`timeout` mantém a edição numa segunda execução. E quando não há nada a mudar o
arquivo **não é reescrito** — reescrever com os mesmos bytes ainda aparece no
`git status`. O `--remove` tira só a nossa entrada, e leva junto o
`"PreToolUse": []` vazio: deixar isso para trás é lixo no arquivo dos outros.

**O `init` não gera regra nenhuma.** Regra gerada é regra que ninguém escolheu,
e um linter que começa reportando coisa que o usuário não pediu é um linter que
ele desliga. O que ele escreve que vale é a linha `$schema` — com o C14 no
lugar, o editor passa a dar completação **e** erro em chave errada. Ele se
recusa a sobrescrever: config é escrita à mão e costuma ser longa.

Verificado no binário, o setup recomendado inteiro do
`AGENT-INTEGRATION.md:246`: `init` → `install-hooks --claude-code` → hook
negando uma escrita inválida com a mensagem que o `ROADMAP.md:57` pede.

```
decision: deny
archwarden: `src/user/create-client.use-case.ts` would break these rules.

  [error] usecase-name — `CreateClient` is declared as `arrow` or `const`
  expected: an export named `CreateClient`

Run `archwarden scaffold src/user/create-client.use-case.ts` for the shape it should have.
```

`cargo mutants` nos dois módulos: 21 mutantes, **zero sobreviventes**.

---

### M8 — `config doctor` `✅`

**Entrada vinda do M7b:** `config doctor` deve avisar quando um
`signature_hint` não combina com o `kind` que a regra exige — hint em estilo
arrow sob `kind: "function"` faz o `scaffold` emitir uma linha que não compila.


**Objetivo:** pegar config errado antes de o usuário culpar a ferramenta.
Dividido em quatro.

---

#### M8a — `config doctor`, checagens sem caminhar o repo `✅`

**Registro** — 2026-07-25

**Correção C16 — três das dez checagens já eram erro duro.** O
`CONFIG.md:333` listava id duplicado, `disable` de id inexistente e preset
declarando `root` como coisas que o doctor reporta. As três são erro no
`extends.rs` desde o M1, e isso é **melhor**: a config nem carrega, então o
erro aparece onde o usuário está olhando, e não num comando separado que ele
pode nunca rodar. Documento corrigido.

**Quatro checagens entregues**, todas respondíveis sem tocar em arquivo:

| Código | O que pega |
|---|---|
| `walk-skip-hides-imports` | D5: `skip_dirs.scope: "walk"` ao lado de boundary rule |
| `unreachable-scope` | D6: escopo inteiramente dentro de um `ignore` |
| `spec-folder-not-allowed` | `spec-pair` olhando pasta que a `structure` proíbe |
| `hint-disagrees-with-kind` | entrada do M7b: hint arrow sob `kind: "function"` |

**Cada achado carrega três coisas: código, frase e correção.** O código é slug
estável para grep e para uma ferramenta decidir; a frase diz o que está errado;
a correção diz o que fazer. Um doctor que só aponta é um doctor que o usuário
lê uma vez.

**Sai com exit 0 mesmo com achados.** São conselhos sobre configuração, não
achados sobre código. Um exit não-zero colocaria escolha deliberada dentro de
um gate de CI, e aí o comando vira coisa que se desliga.

**A checagem de escopo inalcançável erra para o silêncio, de propósito.**
Contenção de globs é indecidível no geral. Ela só afirma cobertura quando o
`ignore` é um `**` sob prefixo literal que também prefixa o escopo — se tiver
glob no prefixo do `ignore`, não conclui nada. Um doctor que gritasse sobre
regras que funcionam é um doctor que ninguém roda.

**A checagem de hint também.** Só reporta o caso inequívoco — hint com `=>` sob
regra que exige exatamente `function`. Regra que aceita `function` ou `arrow`
não é questionada, porque das duas formas o hint está certo para uma delas.

Um mutante sobreviveu e apontou lacuna real: o `governing_structure` casa a
regra de `structure` pelo **escopo**, e nenhum teste tinha mais de uma regra
sobre o mesmo escopo nem uma `structure` sobre escopo diferente. Os dois casos
importam — o primeiro pegaria a regra errada, o segundo inventaria um achado
sobre config correta. Dois testes, **zero sobreviventes**.

---

#### M8b — `config doctor`, checagens contra o repositório `✅`

**Registro** — 2026-07-25

Quatro checagens, a metade lenta — é por isso que o `CONFIG.md` chama o doctor
de mais devagar que o `validate`: ele caminha a árvore e parseia os arquivos das
regras que perguntam sobre conteúdo.

| Código | O que pega |
|---|---|
| `scope-matches-nothing` | escopo apontando para diretório que não existe |
| `pattern-matches-nothing` | regex que não casa nenhum arquivo do escopo |
| `symbol-never-imported` | `call-obligation` com módulo que ninguém importa |
| `only-a-default-export` | D9: arquivo só com default sob regra `naming` |

**A distinção que faz o `symbol-never-imported` valer:** *um* arquivo sem o
import é achado do `check` — é o código que está errado. **Nenhum** arquivo ter
é outra afirmação: o nome do módulo na config provavelmente está errado, e todo
arquivo do escopo está prestes a ser reportado por causa de um typo.

**Dois testes acharam bugs de verdade, não só cobertura.**

1. Escrevi um teste esperando `pattern-matches-nothing` para `user.ts` sob
   `^[a-z]+\.ts$` — que **casa**. Meu próprio comentário no teste já duvidava
   ("wait"). O teste estava errado, o código certo.

2. O sério: o `in_scope` filtrava só pelo **escopo**, ignorando o
   `file_pattern` da regra. Uma `call-obligation` com escopo `src/*` e pattern
   `^route\.post\.ts$` contava também os `route.get.ts` do mesmo diretório —
   e reportava "nenhum arquivo que esta regra cobre importa X" tendo olhado
   arquivos que ela não cobre. **Falso positivo.**

   O conserto foi parar de derivar aplicabilidade: o doctor agora pergunta ao
   `applies_to` da própria engine, o mesmo seam que o `describe` usa. Uma
   segunda implementação de "esta regra cobre este arquivo?" ia acabar
   discordando do checker.

**Um contador virou flag por causa de um mutante.** O `looked_at += 1` sobrevivia
a virar `-=`, e o `[profile]` do workspace não liga `overflow-checks`, então o
`usize` daria a volta em silêncio em vez de estourar. A pergunta é "esta regra
cobriu alguma coisa?", que é um booleano — contador ali só convidava aritmética
que ninguém precisa.

`cargo mutants`: 55 mutantes, **zero sobreviventes**.

**Se a árvore não caminha**, o comando ainda imprime o que a config sozinha já
disse, com uma nota no stderr. Metade da resposta é melhor que nenhuma.
---

#### M8c — `config explain` `✅`

**Registro** — 2026-07-25

O `describe` responde "o que se aplica a este caminho?". Este responde a
direção contrária: "o que esta regra alcança, e o que ela está reportando?".
É o comando para quem escreveu uma regra e não sabe dizer se ela está fazendo
alguma coisa.

```
usecase-name (naming) — error
  applies to: src/*

  Covers 1 path:
    src/user/create-client.use-case.ts

  Flags 1 path:
    src/user/create-client.use-case.ts — the only export is a default, ...
```

**Os flags vêm de um run de verdade, filtrado por id.** Não é uma segunda
avaliação: o que o `explain` mostra é o que o `check` reporta, por construção,
e os dois não têm como divergir. Custa checar todas as regras para mostrar uma
— aceitável num comando que já é o caminho lento, e barato comparado a manter
duas implementações em acordo.

**"Cobre" quer dizer "tem exigência sobre"** — a mesma definição do `describe`.
Uma regra cujo escopo casa um arquivo sobre o qual ela não tem nada a dizer não
o cobre, e listá-lo diria ao usuário que a regra alcança mais do que alcança.

**Regra que não cobre nada diz isso e aponta o `config doctor`.** É justamente
o caso que faz alguém rodar este comando, e o porquê mora no outro.

**Id desconhecido lista os ids reais.** Errar o id — typo, ou confundir o
*kind* da regra com o id dela — é o jeito mais provável de chegar nesse erro, e
a lista é a resposta.

`cargo mutants`: 15 mutantes, **zero sobreviventes**. Cobertura 99,41%.

Uma nota de processo: o `typos` reclamou de `usecase-nmae`, que era typo
*deliberado* num teste. Troquei por `usecase-naming` — id errado que é palavra
válida, e que representa melhor o erro real (confundir kind com id) do que uma
letra trocada.
---

#### M8d — caret exato em erro de config (opção C) `✅`

Decidido no M1: trocar o parse por uma AST com spans e casar o caminho do
`serde_path_to_error` contra ela.

**Registro** — 2026-07-25

**Antes**, uma violação de schema não ganhava caret nenhum — o M1 tinha medido
que a posição do `serde_json` só é confiável para erro de sintaxe, porque num
erro de schema o parser já passou do valor ofensor e o caret acusaria a regra
seguinte. **Agora:**

```
 5 │     { "type": "structure", "id": "second", ..., "allow": ["types"] }
   ·                                                 ───┬───
   ·                                                    ╰── here
```

`jsonc-parser` 0.33 (MIT) dá `Range` em todo nó. O documento é parseado uma
segunda vez e o caminho é caminhado pela AST. Dois parses de um arquivo de
config não é nada — acontece uma vez, no caminho de imprimir um erro.

**O caminho virou estruturado.** Era `String` (`"rules[1]"`); virou
`Vec<PathSegment>`. Um caminho renderizado não pode ser caminhado de volta: uma
chave contendo `.` ou `[` é indistinguível da pontuação que separa os
segmentos.

**Duas precisões, e a diferença importa.** Para "unknown field `allow`" o serde
reporta o **objeto que contém**, porque o campo não faz parte do struct que ele
estava montando — a palavra errada está na mensagem, então o caret vai nela.
Para o resto, o caret vai no nó que o caminho nomeia.

**Descoberta que custou um teste vermelho: o caminho para dentro de uma regra.**
Escrevi um teste esperando `rules[0].must_export.kind` e veio `rules[0]`. O
`Rule` é enum com tag interna (`#[serde(tag = "type")]`), e o serde
desserializa um desses **bufferizando o objeto** e lendo o variant do buffer —
o rastreador de caminho não atravessa essa fronteira.

É a **segunda** vez que o `tag = "type"` cobra: no M4 ele eliminou todo formato
de cache não auto-descritivo, trocando postcard por MessagePack. Vale as duas
vezes — a tag é o que faz o relatório JSON ser contrato que um agente lê — mas o
preço é real e ficou registrado nos dois lugares, para o próximo leitor não
gastar uma tarde procurando o bug.

Na prática o caret ainda cai **na regra certa entre trinta**, que é a pergunta
que o usuário está fazendo. E para campo desconhecido cai na palavra exata.

**O caminho continua na mensagem mesmo com caret.** Caret mostra *onde*;
`rules[1]` diz *o quê*, e quem lê uma falha no log de CI só tem o texto.

`cargo mutants`: 48 no `locate.rs` + `diagnostic.rs`, 11 no `discovery.rs`,
**zero sobreviventes** — depois de matar um que apontou lacuna real: a
delegação de erro de preset (`ExtendsError::Unloadable`) não tinha teste, e sem
ela quem tem typo num preset ouviria só "um preset não pôde ser carregado", sem
nada para abrir.

**Nota lateral:** o `cargo deny` avisa que a permissão `BSD-3-Clause` do
`deny.toml` não corresponde a nenhuma dependência. É anterior a este step e não
falha o check; vale limpar quando alguém passar por lá.

**Pronto quando:** cada checagem tem fixture que a dispara.

**Registro**
> _(pendente)_

---

### M9 — Distribuição `∥` `🟡`

**Objetivo:** instalável sem toolchain (`ARCHITECTURE.md:199-206`).

Paralelizável a partir do M0 — a matriz de cross-compile leva dias de iteração
de CI e não bloqueia nada.

**Tarefas**
- Matriz: macOS x86_64/aarch64, Linux x86_64/aarch64/musl, Windows x86_64.
- Release automatizado em GitHub Releases.
- Shim npm `@archwarden/cli`.
- Metadata de `cargo-binstall`.
- Fórmula homebrew.

**Pronto quando:** as três vias de instalação funcionam a partir de uma tag.
**Não atendido daqui** — ver o registro.

**Registro** — 2026-07-25

**A matriz não cross-compila; cada alvo builda num runner da própria
arquitetura.** Descobri o porquê tentando: `cargo build --target
aarch64-unknown-linux-musl` falha com
`failed to find tool "aarch64-linux-musl-gcc"`. O **`blake3` compila C** para
os caminhos SIMD, então cross-compilar exige um toolchain C por alvo — imagem
docker ou instalação de linker, máquina para manter.

O `ubuntu-24.04-arm` do GitHub tornou a metade arm64 nativa, e o musl só
precisa do `musl-tools` da arquitetura em que já está. Sobrou **um**
cross-compile de verdade: x86_64 macOS a partir de runner arm64, que o
toolchain da Apple resolve sozinho porque o SDK carrega as duas arquiteturas.

Alternativa considerada e descartada: a feature `pure` do `blake3`, que
dispensa o C. O M4 mediu que ler+hashear são 22,8 ms em 10k arquivos — parte
significativa do run —, então desligar SIMD para simplificar o CI seria pagar
com o produto para economizar no build.

**O que eu consegui verificar daqui, e verifiquei:**

- **O empacotamento**, executando os mesmos comandos do workflow: o `.tar.gz`
  contém `archwarden-{version}-{target}/archwarden`, o binário extraído roda,
  e o `shasum -c` do checksum publicado bate.
- **Que o `bin-dir` do `binstall` casa com esse layout** — a URL montada e o
  caminho interno conferidos contra o que o workflow escreve. É o par que mais
  facilmente sai de sincronia, e a falha seria um 404 que ninguém depura pela
  mensagem.
- **O shim npm, ponta a ponta.** Cinco testes nas funções que decidem *o quê*
  baixar (plataforma → triple, detecção de musl, URL, caminho interno), e o
  wrapper rodado contra o binário real: `--version` funciona, `check` devolve
  **exit 1** com achado, o stdin chega no `hook claude-code`, e sem binário
  instalado ele sai **2** com instrução em vez de stack trace. O código de
  saída é a interface — perdê-lo transformaria um gate que falha num que
  passa.
- **O stamper da fórmula do Homebrew**, com seis testes, incluindo contra a
  fórmula **real** deste repositório e não uma fixture: o padrão precisa casar
  a indentação e as aspas dela, e uma fixture que divergisse passaria enquanto
  o release quebrava. Ele recusa placeholder sobrevivente, alvo faltando e
  arquivo de checksum malformado — porque um checksum errado só aparece na
  máquina de um estranho, dias depois.

**Um bug que eu escrevi e o próprio exercício pegou.** A primeira versão do job
da fórmula tinha um heredoc Python indentado dentro de um `for` do shell — que
não termina, porque `<<'PYTHON'` exige o delimitador na coluna zero. Extraí
para `scripts/stamp-formula.py`, o que de quebra tornou a coisa testável. Um
script de release embutido em YAML é um script que ninguém roda até o dia do
release.

**O CI ganhou um job `distribution`** que roda os testes do shim e do stamper
em toda PR. O workflow de release não roda numa PR, mas as duas peças de que
ele depende rodam.

**O que continua não verificado, e não tem como daqui:**

- O build real dos sete alvos. Só o `aarch64-unknown-linux-gnu` é nativo aqui;
  musl não instala sem root, e macOS e Windows não existem nesta máquina.
- O `softprops/action-gh-release`, o `npm publish` e o download que o
  postinstall faz — todos precisam de uma tag de verdade no GitHub.
- O `NPM_TOKEN` no repositório.

**O critério "as três vias funcionam a partir de uma tag" só se fecha com uma
tag.** O `workflow_dispatch` builda os sete alvos e **para antes de publicar**,
o que testa a metade cara sem criar release nenhum — é o ensaio a fazer antes
de qualquer anúncio.

**Revisão depois da pergunta do Henrique ("merge na main já cria os
binários?").** Não: o gatilho é tag, não push. A pergunta fez eu reler o
workflow procurando o que quebraria na primeira execução, e achou duas coisas
de verdade:

1. **`shasum` no runner do Windows.** Sob `shell: bash` ali quem responde é o
   Git for Windows, e o que ele instala varia. Virou `scripts/checksum.py`,
   com 6 testes — incluindo um que entrega o arquivo gerado ao `shasum -c` de
   verdade e confere que passa. Um formato que só concorda consigo mesmo é um
   formato que falha na máquina do usuário.
2. **`npm publish` sem `NPM_TOKEN`.** Falharia *depois* de o release já ter
   sido publicado, deixando o workflow vermelho sobre um release que deu
   certo. Agora avisa e pula.

E uma que eu **achei que era bug e não era**: `[ -n "" ] && VERSION=...` sob
`set -e`. Testei com a linha exata que o GitHub usa
(`bash --noprofile --norc -eo pipefail`) e o bash isenta lista `&&` que não
seja o último comando do script. Reescrevi como `if` mesmo assim, porque a
isenção depende da posição da linha, mas registro que não era defeito.

**Terceira revisão, quando o Henrique perguntou se os sete alvos sairiam
mesmo.** A pergunta certa, e a resposta era não. O `aarch64-unknown-linux-musl`
teria falhado.

Fui ao código do `cc-rs` 1.4 em vez de supor. Dois achados:

1. O prefixo de cross só é aplicado atrás de `get_is_cross_compile()`, e um
   alvo musl num host gnu **conta como cross mesmo com a mesma arquitetura**.
2. A tabela de prefixos trata os dois musl de formas diferentes:
   `"x86_64-unknown-linux-musl" => find_working_gnu_prefix(["x86_64-linux-musl", "musl"])`
   — com fallback para `musl`, que acha o `musl-gcc` do `musl-tools`. Mas
   `"aarch64-unknown-linux-musl" => Some("aarch64-linux-musl")`, **sem
   fallback**. É exatamente o `failed to find tool "aarch64-linux-musl-gcc"`
   que eu tinha visto na tentativa local e atribuído a "falta toolchain".

`CC_<target>` é lido **antes** dessa tabela, e num runner da mesma arquitetura
o `musl-gcc` do `musl-tools` *é* o compilador certo. Verifiquei o mecanismo
localmente apontando `CC_aarch64_unknown_linux_gnu` para um binário
inexistente e vendo o `blake3` reclamar dele por nome — o override vence a
derivação. Está setado para os dois alvos musl, não só o que precisa, para que
nenhum dependa de uma tabela de lookup no crate de outra pessoa.

**Risco restante, que não dá para verificar daqui:** os runners
`ubuntu-24.04-arm` são gratuitos em repositório **público** — o archwarden é —,
então devem resolver. O `fail-fast: false` garante que um alvo que falhe não
derrube os outros seis.

**Versão fixada em 0.1.0** (workspace, `npm/package.json`, fórmula). O caminho
de empacotamento inteiro foi exercitado nessa versão no alvo nativo: arquivo
gerado, checksum escrito, `shasum -c` aceita, binário extraído em
`archwarden-0.1.0-aarch64-unknown-linux-gnu/archwarden` — que é exatamente o
`bin-dir` que o `binstall` procura — e ele responde `archwarden 0.1.0`.

---

## Follow-ups pós-v0

- `.md` no walk e regras que operem sobre ele (D10).
- Reavaliar `dashmap` se algum estágio precisar de estado compartilhado (D2).
- `archwarden-lsp` (v1, `ROADMAP.md:68`).
