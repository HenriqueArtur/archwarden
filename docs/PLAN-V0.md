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

### M5 — Resolver + `import-boundary`

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

#### M5c — `import-boundary` `⬜`

- `rules`: `forbid_import_from`, `must_import_from`, `except`,
  `include_type_only` + `describe_expectation`.
- `engine`: ligar `resolve_imports` no runner (`ImportResolver` construído uma
  vez por run, só quando alguma engine pede), `Outcomes` no `Report`.
- `cli`: relatar imports não resolvidos — uma boundary rule que não enxergou
  nada é um relatório limpo mentindo.

---

#### M5d — Tier 3 `⬜`

- Harness differential vs `dependency-cruiser`,
  `tests/differential/known-divergences.md`.

**Pronto quando:** Tier 3 roda contra o Flowmaatik sem divergência não
justificada.

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
- **Caret exato em erro de config (opção C).** Trocar o parse por uma AST com
  spans (`jsonc-parser` ou equivalente) e casar o caminho do
  `serde_path_to_error` contra ela. Fica aqui porque o doctor precisa de span
  para "campo desconhecido `allowed_subfolder`, você quis dizer
  `allowed_subfolders`?" de qualquer forma, e fazer as duas coisas juntas evita
  mexer na camada de diagnóstico duas vezes. Decidido em 2026-07-25.
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
