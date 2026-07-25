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
- **`cargo-llvm-cov`** — piso de 90% em `core`/`config`/`rules`, 70% no workspace.
- **Armadilha do `insta`:** snapshot esperado é escrito **à mão antes**
  (inline snapshot). `cargo insta review` só para revisar mudança intencional
  em snapshot existente — nunca para criar o primeiro.

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

#### M1b — `config` + `resolver` mínimo + CLI `⬜`

**Objetivo:** a fatia vertical — do JSON no disco até um comando que responde.

**Tarefas**
- `config`: tipos de wire format com `Deserialize` + `JsonSchema` — enum `Rule`
  de 5 variantes (D14), array `rules` de topo + `modules[].rules`, helper
  `OneOrMany` para todo campo de glob.
- `config`: discovery subindo do CWD (ADR#4), `--config` override.
- **`resolver`: trait `Resolver` + `OxcResolver` configurado só para resolução
  de pacote** (achar `<pkg>/package.json` e o entry point). Antecipado do M5
  por causa de D7 — ver nota abaixo.
- `config`: `extends` — path relativo + pacote npm via `resolver`, merge,
  escalares (local vence), `disable`, erro se preset declara `root`.
- `config`: lowering para `core::CompiledConfig` (compila globs e regexes).
- `config`: erro de regex com lookahead com mensagem explicativa (D3).
- `xtask gen-schema` → `schema/v0.json`.
- `cli`: `clap`, `archwarden config validate`, exit codes 0/1/2.
- Tier 1 em tudo. `proptest`: config loading nunca panica (`TESTING.md:150`).
- **Remover `--no-tests=pass` do job `test` em `.github/workflows/ci.yml`.**
  Foi muleta do M0 (workspace sem teste algum). Com TDD obrigatório, "nenhum
  teste rodou" tem que quebrar o build.
- ✅ **Piso de cobertura ligado** no job `coverage`, com os números que o
  Henrique fixou em 2026-07-25: **100% no `archwarden-core`**, **95% no
  workspace** (meta 100%). Dois invocações do `llvm-cov`, uma por piso.

**Pronto quando:** `archwarden config validate` roda contra config válido e
inválido com exit code e mensagem `miette` corretos; schema gerado valida os
exemplos do `CONFIG.md`; `extends` resolve preset em npm, pnpm e yarn PnP.

**Nota sobre o DAG (D7):** `config` depende de `resolver`, que depende só de
`core` — acíclico. O crate `archwarden-resolver` nasce aqui com a configuração
mínima de resolução de pacote; a configuração TS-aware (`tsconfig.paths`,
extensões, condicionais de `exports`) é do M5. Mesmo crate, duas configurações,
entregues em momentos diferentes.

**Registro**
> _(pendente)_

---

### M2 — Walk + `structure` + `spec-pair` `⬜`

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

**Registro**
> _(pendente)_

---

### M3 — Parser + `naming` `⬜`

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

**Registro**
> _(pendente)_

---

### M4 — Cache `⬜`

**Objetivo:** bater o critério warm do `ROADMAP.md:48-51`.

**Tarefas**
- `cache`: store `redb`, duas tabelas — `facts[content_hash]` e
  `findings[content_hash + rules_hash + resolution_epoch]` (C5).
- `cache`: `resolution_epoch` = hash de `tsconfig*.json` + `package.json` +
  lockfile (C4).
- `cache`: versionamento de formato (ADR#3), invalidação total em bump.
- `engine`: probe antes de parse, flush em lote no fim.
- `benches/`: criterion cold/warm; baseline registrada com a máquina (D13).

**Pronto quando:** warm run mede-se em `criterion` e a invalidação por mudança
de `tsconfig.paths` tem teste.

**Registro**
> _(pendente)_

---

### M5 — Resolver + `import-boundary` `⬜`

**Objetivo:** grafo de imports próprio (ADR#7).

**Tarefas**
- `resolver`: configuração TS-aware do `OxcResolver` já existente do M1 —
  `tsconfig.paths`, extensões, condicionais de `exports`, workspaces.
- `resolver`: `InMemoryResolver` para fixture (`ARCHITECTURE.md:98`).
- `parser`: `ImportFact` com marcação type-only (`RULES.md:123-126`).
- `engine`: montagem do grafo, índice reverso (`ARCHITECTURE.md:141-144`).
- `rules`: `import-boundary` — `forbid_import_from`, `must_import_from`,
  `except`, `include_type_only` + `describe_expectation`.
- Tier 3: harness differential vs `dependency-cruiser`,
  `tests/differential/known-divergences.md`.

**Pronto quando:** Tier 3 roda contra o Flowmaatik sem divergência não
justificada.

**Depende de:** resolução de C2 (shape do `import-boundary` no config).

**Registro**
> _(pendente)_

---

### M6 — `call-obligation` `⬜`

**Objetivo:** a regra que nenhum outro tool faz.

**Tarefas**
- `parser`: `CallFact`, incluindo method chains (`Event.save`).
- `rules`: call-graph intra-arquivo a partir dos exports top-level
  (`CONFIG.md:197-200`); checagem de `imported_from`; falha específica
  "expected import missing" (`RULES.md:153-155`) + `describe_expectation`.

**Pronto quando:** obrigação satisfeita via helper local é detectada;
cross-file continua fora de escopo, com mensagem clara.

**Registro**
> _(pendente)_

---

### M7 — Superfície de agente `⬜`

**Objetivo:** ADR#9 completo — informante, não só gate.

**Tarefas**
- `cli`: `describe <path>` (text + JSON), reusando o matcher, sem parse.
- `cli`: `scaffold <path>` consumindo `describe_expectation` de cada regra.
- `cli`: `agent-guide --format markdown|json --scope <glob>`, determinístico.
- `cli`: `check --file <path>` com `"skipped": [...]` explícito (C6).
- `cli`: `install-hooks --claude-code [--remove]`, idempotente.
- `cli`: `init`.
- Tier 2 para cada comando.

**Pronto quando:** `describe`/`scaffold` respondem <50ms warm
(`ROADMAP.md:54`); hook do Claude Code bloqueia escrita inválida com mensagem
que identifica regra e correção (`ROADMAP.md:55-57`).

**Registro**
> _(pendente)_

---

### M8 — `config doctor` `⬜`

**Objetivo:** pegar config errado antes de o usuário culpar a ferramenta.

**Tarefas**
- Todas as checagens de `CONFIG.md:227-233`.
- Avisos novos das decisões: `skip_dirs.scope:"walk"` + `import-boundary` (D5);
  `roots` coberto por `ignore` (D6); preset declarando `root` e `disable` de id
  inexistente (D7); arquivo só com default export sob regra `naming` (D9).
- `config explain <rule-id>`.

**Pronto quando:** cada checagem tem fixture que a dispara.

**Registro**
> _(pendente)_

---

### M9 — Distribuição `∥` `⬜`

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

**Registro**
> _(pendente)_

---

## Follow-ups pós-v0

- `.md` no walk e regras que operem sobre ele (D10).
- Reavaliar `dashmap` se algum estágio precisar de estado compartilhado (D2).
- `archwarden-lsp` (v1, `ROADMAP.md:68`).
