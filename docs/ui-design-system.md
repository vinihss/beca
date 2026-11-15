# Sistema de design da UI do beca (plano e decisões)

> **Estado:** Fase 0 detalhada; identidade orientada a **Material 3 (baseline roxo #6750A4)** prototipada no Figma. Próximo passo: implementação Fases 1–3 (tokens/normalização/catálogo) com mapeamento M3.  
> **Data:** 12 nov 2026 (atualizado em 12 nov 2026 — adoção M3)  
> **Escopo deste documento:** registrar o diagnóstico, as decisões, a arquitetura proposta e o roadmap para amadurecer a UI do beca (tokens, primitivas, catálogo/storybook nativo e identidade), servindo como fonte de verdade do plano.

## 1. Contexto e diagnóstico (baseline)

### 1.1 Estrutura atual (`beca-ui`)

- `crates/beca-ui/ui/`: `theme.slint`, `library.slint`, `components/` (7), `bar/` (6), `search/`, `voice/` (5 janelas/componentes).
- `crates/beca-ui/src/`: `theme.rs` (tokens + parse/aplicação tipada e via interpreter), `ui_loader.rs`, `voice/*`.
- `library.slint` exporta hoje principalmente janelas (`Voice*`, `SearchWindow`, `BarWindow`); as primitivas de `components/` não formam um catálogo público estável.

Componentes base existentes (`ui/components/`): `icon_button.slint`, `pill_button.slint`, `media_button.slint`, `desktop_button.slint`, `info_card.slint`, `waveform_view.slint`, `oscilloscope_view.slint`. Widgets mais específicos vivem em `ui/bar/`, `ui/search/`, `ui/voice/`.

### 1.2 Tokens/tema atuais

`ui/theme.slint` (`export global Theme`) e `src/theme.rs` (`ThemeTokens`) espelham:

- `dark-mode` (bool), `radius` (length), `icon-font-family` (string).
- Cores/brushes: `background`, `text-main`, `card-bg`, `card-border`, `surface`, `surface-alt`, `border`, `text-strong`, `accent`, `accent-success`, `accent-danger`, `recording`.

Ponte Rust: `ThemeTokens` (default, derivação de `voice::VoiceTheme`, `apply_typed`/`apply_interpreter`) com fallback para cores inválidas (comportamento a preservar).

**Lacunas observadas:**
- Sobreposição/ambiguidade semântica: `background` vs `surface`, `text-main` vs `text-strong`, `card-bg` vs `surface`, `border` vs `card-border`; mistura de `color`/`brush` para conceitos similares.
- Ausência de escalas explícitas: tipografia, espaçamento, raio, borda/contorno, opacidade/estados, elevação/sombra e motion.
- Uso parcial/inconsistente nas primitivas: literais de cor/raio/spacing/estado (ex.: `DesktopButton` hardcoda cores e estados; raios/`padding` literais em vários componentes; `InfoCard` declara `border-size` sem aplicação consistente de borda/token).
- Contratos de componente variáveis (`active`, `enabled`, `muted`, callbacks) sem convenção única de variantes/tamanhos/estados.

**Conclusão:** há base reutilizável, mas ainda sem sistema consolidado (tokens semânticos + primitivas normalizadas + padrões de composição + catálogo vivo).

## 2. Decisões confirmadas

| # | Decisão | Status |
|---|---|---|
| D1 | **Escopo MVP (Fase 1):** *foundations + primitivas* (componentes base). Widgets/padrões compostos de domínio (`bar`, `launcher`, `voice`, `search`) ficam fora do MVP e entram como casos de composição após primitivas estáveis. | Confirmado |
| D2 | **Forma do catálogo/storybook:** alvo **separado** do shell (não integrado ao runtime do `beca`), consumindo os mesmos componentes/tokens do `beca-ui`. | Confirmado |
| D3 | **Identidade visual:** **Material 3 (baseline roxo, seed #6750A4)** adotado como referência/direção, com adaptação ao contexto do beca/shell a validar no catálogo. A paleta/linguagem atual passa a ser legado a migrar. | Confirmado (atualizado — ver §12) |

**Defaults adotados para a Fase 0 (salvo revisão explícita):**
- **Crate do catálogo:** `crates/beca-ui-catalog` (binário `beca-ui-catalog`), membro do workspace, dependendo de `beca-ui`.
- **`WaveformView`/`OscilloscopeView`:** fora do núcleo do MVP; normalização profunda adiada. Podem constar em subseção “visual/especializada” do catálogo apenas após tokenização do contrato (não bloqueia a Fase 1).
- **Interatividade do catálogo:** galeria (majoritariamente estática) de estados/variantes/casos de uso + seletor básico de tema/preset; sem playground completo de props no MVP.
- **Fonte de verdade dos tokens (proposta):** definição/derivação de tokens no Slint (`Theme`/módulos de tokens) como contrato de UI; overrides de personalização injetados pelo Rust/config via `ThemeTokens` (evoluindo o espelhamento atual), preservando compatibilidade e fallbacks.

## 3. Objetivos e princípios

- **Enxuto e minimalista:** poucas primitivas bem resolvidas; tokens semânticos claros; evitar variantes e camadas desnecessárias.
- **Moderno e consistente:** estados/interações previsíveis; hierarquia visual expressa por tokens, não por literais.
- **Alta performance:** UI reativa/compilada (Slint); catálogo renderiza cenas pequenas e sob demanda por seção, sem custo para o shell em produção.
- **Personalizável:** tema dirigido por tokens (presets + override de config), com ponte estável Slint↔Rust.
- **Fonte de verdade viva:** catálogo e shell consomem os **mesmos** componentes e tokens do `beca-ui` (sem cópias/divergência).
- **Documentação junto ao código:** foundations, contratos de componente, estados e casos de uso materializados no catálogo.

## 4. Arquitetura de tokens (proposta)

### 4.1 Camadas

1. **Primitives (escala, valores brutos):** paleta neutra + accent, escala de spacing, radius, tamanhos tipográficos/ícones, durações/curvas. Sem significado de uso direto nos componentes.
2. **Semantic (contrato consumido pelos componentes):** papéis estáveis — fundo/superfície, texto, borda, accent, status, estados e foco — expostos via `export global Theme` (e/ou módulos dedicados), idealmente com propriedades derivadas (`out property`) a partir de primitives + modo.
3. **Component tokens (opcional, pontual):** apenas quando uma primitiva exigir ajuste específico, derivado de semantic/primitive (evitar literais e “vazamento”).

Organização sugerida (nomes a fechar na Fase 0): `ui/tokens/primitives.slint`, `ui/tokens/semantic.slint` (ou evolução do `theme.slint`) e eventual `ui/foundations/`.

### 4.2 Grupos mínimos

| Grupo | Conteúdo mínimo sugerido |
|---|---|
| Cor/semântica | `bg`, `surface`, `surface-alt/raised`, `border`, `border-subtle`, `text-primary`, `text-secondary`, `text-on-accent`, `accent` (+ variações hover/pressed quando derivadas), `success`, `danger`, `warning` (se aplicável), `focus-ring` |
| Tipografia | família(s), escala curta (ex.: `sm/md/lg`), pesos (regular/medium/semibold), line-height básico |
| Espaçamento | escala (ex.: `xs/sm/md/lg/xl`, base 4/8) para padding/gap |
| Raio | `sm/md/lg/pill` (consolidar valores hoje literais) |
| Borda/contorno | largura padrão + cor/estilo de foco |
| Opacidade/estado | `disabled`, overlays/cores derivadas de hover/pressed/active |
| Motion | durações/curvas discretas (ex.: fast/base), uso funcional |
| Ícone | `icon-font-family`, tamanhos `sm/md/lg` |

**Consolidação:** mapear/remover duplicatas atuais (`background`/`surface`, `text-main`/`text-strong`, `card-*`/`surface|border`) para nomes semânticos únicos; padronizar `color` vs `brush` conforme intenção.

### 4.3 Modos e personalização

- **Modos:** `system / light / dark` via `std-widgets.slint` `Palette.color-scheme` + preferência explícita (`ThemePreference`), com tokens semânticos derivados (padrão recomendado em Slint).
- **Personalização:** presets + overrides (ex.: accent/superfícies) mapeados aos tokens semânticos; valores injetados via config/Rust preservando fallback para inválidos.
- **Ponte Rust:** evoluir `ThemeTokens`/`apply_typed`/`apply_interpreter` para refletir o contrato semântico (desacoplado de campos ad-hoc de voz), mantendo paridade e compatibilidade de configuração existente.
- **Identidade:** valores atuais = baseline; novos presets/direções são validados no catálogo (D3) antes de se tornarem default.

## 5. Primitivas do MVP (escopo e contrato)

### 5.1 Inventário alvo (Fase 1)

**Normalizar (existentes em `ui/components/`):**
- `IconButton` — ação icônica.
- `PillButton` — ação compacta ícone/label.
- `MediaButton` — ação circular/primária.
- `DesktopButton` — ação textual padrão (**prioridade:** remover hardcodes de cor/estado).
- `InfoCard`/Surface — container/superfície com título e `@children` (papel e borda/token claros).

**Candidatas de fundação (incluir se caírem naturalmente do inventário, mantendo enxuto):**
- Convenção/componente fino de **Text/Label** por estilos semânticos (evitar tipografia solta).
- **Divider/Separator** e **Badge/Indicator** mínimos, se necessários às stories das primitivas.

**Adiado (Fase 1):** `WaveformView`, `OscilloscopeView` (primitivas visuais especializadas; normalização/tokenização profunda fora do núcleo — ver D defaults).

### 5.2 Contrato comum

- API consistente: variantes e tamanhos quando aplicável; estados `enabled/disabled`, `hover/pressed/focus` coerentes; `active/toggled` onde fizer sentido.
- Conteúdo/slots e callbacks previsíveis (`clicked` etc.).
- **Somente tokens semânticos/primitives** para cor, tipografia, spacing, radius, borda, estado e motion (proibido literal hardcoded, salvo exceção documentada).
- Acessibilidade mínima: contraste adequado, alvo de interação razoável, indicação de foco onde aplicável.
- Exportação pública das primitivas em `beca-ui` (ex.: via `library.slint`/módulo dedicado) para consumo pelo catálogo sem acoplamento às janelas do shell.

## 6. Catálogo separado (arquitetura)

### 6.1 Estrutura

- **Novo crate:** `crates/beca-ui-catalog` (member do workspace) com binário `beca-ui-catalog`.
  - Dependências: `beca-ui` (+ `slint`/`slint-build` conforme necessário).
  - `src/main.rs`: inicializa e exibe janela nativa Slint de desenvolvimento (isolada do layer-shell/daemon do shell).
  - `ui/` do catálogo (ex.: `ui/catalog.slint`): navegação por seções e área de preview.
- **Reuso:** importa tokens e primitivas exportados por `beca-ui`; qualquer evolução de componente/token reflete no catálogo por ser a mesma implementação.

Esboço (ilustrativo, a consolidar na Fase 0):
```text
crates/beca-ui/
  ui/
    tokens/            # primitives + semantic (ou theme evoluído)
    primitives/        # ou components/ normalizado e exportado
    ...
    library.slint      # exports: Theme/tokens + primitivas (+ janelas existentes)

crates/beca-ui-catalog/    # separado
  Cargo.toml
  build.rs                 # se compilar .slint do catálogo
  src/main.rs
  ui/catalog.slint         # foundations + stories de primitivas
```

### 6.2 Conteúdo do MVP do catálogo

- **Foundations:** swatches de paleta/cores semânticas, tipografia (escala/pesos), spacing, radius, borda/foco, ícones e motion/estados — reagindo ao tema ativo.
- **Primitives:** uma seção por primitiva com variantes/estados (default, hover/pressed/focus quando aplicável, disabled, active/toggled, tamanhos) e casos de uso curtos representativos.
- **Identidade:** espaço para comparar baseline vs direção(ões) propostas (paleta/tipo/raio/densidade) e apoiar a decisão (D3).
- **Tema:** seletor simples (system/light/dark e/ou preset/accent) para inspecionar tokens e personalização.

### 6.3 Execução e performance

- Execução de dev: `cargo run -p beca-ui-catalog`.
- Isolado de `beca daemon`/superfícies do shell; compatível com `slint-viewer` para iteração pontual de `.slint`.
- Manter cenas/árvores enxutas; evitar trabalho contínuo desnecessário nos previews.

## 7. Processo de revisão da identidade (D3)

1. **Auditar baseline:** representar no catálogo a identidade atual tokenizada (cores, tipografia, radius, densidade, contraste) e apontar inconsistências.
2. **Definir princípios:** minimalista, contraste funcional, acentos contidos, superfícies discretas, legibilidade, densidade coerente, motion funcional.
3. **Propor direções:** 1–2 presets comparáveis (ex.: evolução refinada do atual vs direção neutra/minimal alternativa) variando paleta neutra+accent, tipografia e radius/densidade.
4. **Validar e decidir:** avaliar no catálogo (inclusive leitura sobre fundos típicos do desktop/Wayland) e só então promover valores a defaults do sistema de tokens/config.

## 8. Roadmap de execução

| Fase | Entregas | Critério de pronto |
|---|---|---|
| **0. Inventário e decisões** | Mapa de usos de tokens/literais por primitiva; taxonomia semântica exata (`atual → proposta`); fonte de verdade Slint↔Rust/config fechada; lista final de primitivas/estados/stories do MVP (inclui/descarta visual-especializadas); nome/estrutura do crate do catálogo; direções de identidade a prototipar | Contrato de tokens e escopo exato acordados, base para implementar sem ambiguidade |
| **1. Tokens/foundations** | Camadas primitives+semantic, `Theme` consolidado (system/light/dark + preferência), ponte `theme.rs`/`ThemeTokens` alinhada, foundations exibidas no catálogo | Sem duplicação semântica; componentes aptos a migrar |
| **2. Normalização das primitivas** | Primitivas do MVP refatoradas para tokens/estados consistentes, hardcodes removidos, exports públicos em `beca-ui` | Cada primitiva cobre os estados definidos usando apenas tokens; janelas existentes preservadas |
| **3. Catálogo MVP** | `beca-ui-catalog` navegável (foundations + primitivas + estados/casos de uso) com seletor básico de tema/preset | Executa separado, reflete componentes reais, leve |
| **4. Identidade e diretrizes** | Identidade escolhida (presets/valores finais), guia curto de convenções (criar primitiva, usar tokens, adicionar story, regras de estado/motion/acessibilidade) | Decisão visual registrada e aplicável; pronto para Fase 2 (widgets/padrões do shell) |

## 9. Riscos e cuidados

- **Regressão visual no shell:** primitivas são usadas por bar/voice/launcher/search — migrar incrementalmente e validar superfícies existentes a cada passo.
- **Compatibilidade de config/tema:** manter defaults e fallback de valores inválidos na evolução de `ThemeTokens`/tema (comportamento já estabelecido).
- **Escopo:** evitar crescimento do MVP; primitivas visuais especializadas só entram se tokenização couber sem bloquear o núcleo.
- **Exports/acoplamento:** expor primitivas/tokens para o catálogo sem quebrar consumidores atuais das janelas em `library.slint`.
- **Desempenho do catálogo:** preferir galeria enxuta; evitar previews custosos contínuos.

## 10. Referências

- Slint — temas/tokens/globais, `Palette`/`ColorScheme`, live-preview (`slint-viewer`): documentação oficial (`/slint-ui/slint`, `docs.slint.dev`).
- Ecossistema de referência: `slint-ui/material-components` (sistema + galeria), `slint-kit`.
- Práticas de design system/storybook: foundations (tokens), catálogo alinhado ao código, estados/variantes e casos de uso como documentação viva.

## 11. Fase 0 — detalhamento executável

> Detalhamento aprovado para guiar a implementação das fases seguintes. Ajustes pontuais devem ser registrados neste documento ao serem decididos.

### 11.1 Inventário de primitivas alvo (MVP)

Levantamento das primitivas em `crates/beca-ui/ui/components/` selecionadas para normalização no MVP (baseline atual).

| Primitiva | Arquivo | Papel / uso | API (props, callbacks, slots) | Tokens `Theme` usados | Literais / hardcodes | Estados atuais | Ações de normalização |
|---|---|---|---|---|---|---|---|
| `IconButton` | `ui/components/icon_button.slint` | Ação icônica compacta | `icon:string`, `size:length=30px`, `active:bool`, `enabled:bool`, `clicked()` | `surface-alt` (hover bg), `accent` (ativo), `text-main` (ícone) | `border-radius: 6px`, `opacity: 0.45` (disabled), `font-size: 16px`, cursor pointer | default, hover (bg), active (cor), disabled (opacidade) | Trocar radius/opacity/font-size por tokens; definir estado `focus`/`pressed`; derivar cores de estado de tokens semânticos; padronizar tamanhos (`sm/md/lg`) |
| `PillButton` | `ui/components/pill_button.slint` | Ação compacta com ícone/label | `icon:string`, `label:string`, `icon-color:brush=Theme.accent`, `active:bool`, `enabled:bool`, `clicked()` | `surface-alt` (hover/active bg), `accent` (default icon-color), `text-main` (label), `icon-font-family` | `min-width: 56px`, `min-height: 24px`, `border-radius: 6px`, `opacity: 0.45`, `padding: 10px`, `spacing: 6px`, `font-size: 14px/13px`, `font-weight: 600`, cursor pointer | default, hover/active (bg), disabled | Spacing/tipografia/radius/opacity via tokens; estados `pressed/focus`; variante de ênfase (neutra/accent) em vez de `icon-color` solto; consistir label/ícone opcionais |
| `MediaButton` | `ui/components/media_button.slint` | Ação principal/circular (mídia/controle) | `icon:string`, `label:string`, `enabled:bool`, `accent:brush=Theme.accent`, `size:length=48px`, `clicked()` | `accent` (bg), `text-strong` (conteúdo), `surface-alt` (disabled bg) | `width: 118px` (c/ label), `height/2` radius, `opacity: 0.45`, variações `accent.darker(20%)`/`brighter(10%)`, `padding: 14px`, `spacing: 0/6px`, `font-size: size*0.38` e `13px`, `font-weight: 600` | default, hover, pressed, disabled | Formalizar derivação de estados a partir de tokens (accent/overlay) em vez de percentuais ad-hoc; tokens de spacing/tipografia/opacity; contraste `text-on-accent`; variantes tamanho/forma (icon-only, com label) |
| `DesktopButton` | `ui/components/desktop_button.slint` | Botão textual padrão | `text:string`, `enabled:bool`, `clicked()` | `radius` (parcial) | **Hardcodes de cor:** bg `#007ACC`, hover `#0088E6`, pressed `#005999`, disabled `#007ACC`+opacity, texto `white`; `height: 38px`, `min-width: 96px`, `opacity: 0.45` | default, hover, pressed, disabled (visualmente fraco) | **Prioridade:** substituir todas as cores/estados por tokens semânticos (`accent`, `accent-hover/pressed`, `text-on-accent`, `disabled`); adicionar `focus`; normalizar tamanho/padding/tipografia/radius via tokens; considerar variantes (primary/secondary/ghost) se enxuto |
| `InfoCard` / `Surface` | `ui/components/info_card.slint` | Container/superfície com título e conteúdo | `title:string`, `border-size:length=1px`, `@children` | `card-bg` (bg), `text-main` (título) | `border-radius: 8px`, `padding: 16px`, `spacing: 8px`, `font-weight: 700`; `border-size` declarado mas **não aplicado** (sem borda/cor) | estático (container) | Unificar papel como `Surface/Card`: aplicar borda/raio/spacing/tipografia por tokens (`surface`, `border`, `radius`, `text-primary/secondary`), expor variante (plana/borda/elevada se aplicável) e uso real de `border-size`/cor ou remover prop ambígua |

**Fora do núcleo do MVP (adiados, Fase 1):** `ui/components/waveform_view.slint`, `ui/components/oscilloscope_view.slint` — primitivas visuais especializadas; listar em subseção “visual/especializada” do catálogo apenas após tokenização/normalização do contrato. `ui/components/` hoje também contém apenas estes 7 arquivos; o MVP normaliza os 5 acima.

### 11.2 Mapeamento de tokens (`atual → proposto`)

Objetivo: contrato **semântico** único, sem duplicatas, consumido pelas primitivas. Valores iniciais preservam o baseline (D3: identidade a reavaliar) salvo onde indicado.

| Token atual (`Theme`) | Tipo atual | Uso/problema | Token semântico proposto | Observação |
|---|---|---|---|---|
| `dark-mode` | bool | alternância implícita | `preference`/`mode` + derivado `dark` | alinhar a `system/light/dark` (`Palette.color-scheme` + override) |
| `background` | color | base da app; sobrepõe `surface` | `color.bg` | fundo de aplicação/janela |
| `surface` | brush | superfície genérica; sobrepõe `card-bg` | `color.surface` | superfície padrão de componentes |
| `card-bg` | color | duplicata de superfície | (remover como token público) → `color.surface` | migrar consumidores |
| `surface-alt` | brush | superfície alternativa/hover | `color.surface-alt` (ou `surface-raised`/estado derivado) | uso explícito para realce/hover containers |
| `text-main` | color | texto principal; sobrepõe `text-strong` | `color.text-primary` | texto padrão |
| `text-strong` | brush | texto enfatizado/on-accent | `color.text-primary-strong` e/ou `color.text-on-accent` | separar “ênfase” de “texto sobre accent” |
| `card-border` | color | duplicata de borda | (remover) → `color.border` | migrar consumidores |
| `border` | brush | borda genérica | `color.border` (+ `color.border-subtle` se necessário) | unificar |
| `accent` | brush | ação/realce | `color.accent` | base de ações/ativos |
| `accent-success` | brush | status sucesso | `color.status-success` | status semântico |
| `accent-danger` | brush | status perigo/erro/recording-off | `color.status-danger` | status semântico |
| `recording` | brush | estado de gravação (domínio voz) | `color.status-recording` (ou mapear a `danger/accent`) | manter se ainda necessário ao domínio; documentar |
| `radius` | length | raio único | escala `radius.sm/md/lg/pill` | consolidar 6/8px/circular |
| `icon-font-family` | string | fonte de ícones | `font.icon-family` | manter, complementar com tamanhos de ícone |

**Tokens semânticos novos (lacunas a preencher):** `color.text-secondary` (texto secundário/dim), `color.text-on-accent`, `color.status-warning` (opcional), `color.focus-ring`, estados derivados (`accent-hover`, `accent-pressed`, overlays/`state-hover|pressed`), `opacity.disabled`, escalas `space.*` (xs/sm/md/lg/xl), `type.*` (tamanhos/pesos/line-height), `border.width`, `icon.size.*`, `motion.duration/curve.*` (mínimo).

**Camada primitives (proposta):** paleta neutra (ex.: `neutral-0..950`) + paleta accent + escalas numéricas (`space-1..n`, `radius-*`, `font-size-*`, `duration-*`). Semantic referencia primitives e resolve por modo (`light/dark`).

**Ponte Rust/config (`crates/beca-ui/src/theme.rs`, `ThemeTokens`):**
- Evoluir o struct para o **contrato semântico** (renomear/mapear campos duplicados; adicionar `text-secondary`, `text-on-accent`, `focus-ring`, `warning` opcional, e preferencialmente escalas/radius/opacity necessários) mantendo conversão a partir de `voice::VoiceTheme`/config com **fallback para defaults** em valores inválidos.
- Preservar `apply_typed`/`apply_interpreter` atualizando nomes de propriedades do global `Theme`; manter compatibilidade de chaves de config existentes via aliases/mapeamento durante migração.
- **Fonte de verdade (decisão de Fase 0):** tokens e derivações de modo vivem no Slint; Rust injeta overrides de personalização vindos de config. (Registrar ajuste aqui se a Fase 1 exigir inversão.)

### 11.3 Primitivas, variantes, estados e stories do MVP

Lista final de primitivas do MVP e o que o catálogo deve cobrir para cada uma.

| Primitiva | Variantes (MVP) | Tamanhos | Estados | Stories (casos de uso) |
|---|---|---|---|---|
| `IconButton` | default, `active/toggled` | `sm/md` (ex.: 24/30/36) — definir escala | default, hover, pressed, focus, disabled, active | ícone isolado; toggle ativo/inativo; em barra de ações; desabilitado |
| `PillButton` | neutra, accent/ênfase (ícone ou label colorido por token) | `md` (altura compacta) | default, hover, pressed, focus, disabled, active | só ícone; ícone+label; ativo (seleção); desabilitado |
| `MediaButton` | icon-only (circular), com label (pill) | `sm/md/lg` (ex.: 40/48/56) | default, hover, pressed, focus, disabled | controle de mídia icon-only; ação principal com label; desabilitado |
| `DesktopButton` | `primary` (accent) — base; `secondary/ghost` só se trivial no mesmo contrato | `md` (default) | default, hover, pressed, focus, disabled | ação primária; ação secundária/neutra (se variante incluída); desabilitado; largura mínima/conteúdo |
| `Surface` (evolução de `InfoCard`) | plana, com borda (e elevada opcional se tokenizada) | — (container) | default (container estático); foco não se aplica ao container em si | card com título+conteúdo; superfície sem título; agrupamento de conteúdo (`@children`) |

**Foundations (stories obrigatórias):** paleta primitives + semântica (light/dark), tipografia (escala/pesos), spacing, radius, borda/foco, ícones/tamanhos, motion/opacidade de estado, visão da identidade (baseline vs direções, §11.5).

**Critério de aceite por primitiva:** renderiza todos os estados/variantes definidos **usando apenas tokens** (sem literais de cor/spacing/radius/tipografia/estado), com comportamento de interação consistente.

### 11.4 Esqueleto de arquivos e crate

#### 11.4.1 Workspace e novo crate `beca-ui-catalog`

- Adicionar membro em `[workspace].members` (`Cargo.toml` raiz): `"crates/beca-ui-catalog"`.
- Novo crate `crates/beca-ui-catalog/`:
  - `Cargo.toml`: `name = "beca-ui-catalog"`, `edition.workspace = true`, `version.workspace = true`; deps `beca-ui = { path = "../beca-ui" }`, `slint.workspace = true`; `[build-dependencies] slint-build.workspace = true`; bin `beca-ui-catalog` (`src/main.rs`).
  - `build.rs`: compila o entrypoint do catálogo (ex.: `slint_build::compile("ui/catalog.slint")`, análogo ao padrão do workspace).
  - `src/main.rs`: inclui módulos gerados pelo Slint (`slint::include_modules!()` ou equivalente ao `build.rs`), instancia e executa a janela de catálogo (janela Slint nativa, sem layer-shell).
  - `ui/catalog.slint`: shell do catálogo com navegação (seções Foundations / Primitives / Identidade) e área de preview; importa tokens e primitivas de `beca-ui`.

Estrutura ilustrativa:
```text
crates/beca-ui-catalog/
  Cargo.toml
  build.rs
  src/
    main.rs
  ui/
    catalog.slint
    sections/           # opcional: foundations.slint, primitives.slint, identity.slint
```

Execução: `cargo run -p beca-ui-catalog`.

#### 11.4.2 Evolução de `beca-ui` (exports e organização)

- **Tokens:** introduzir `ui/tokens/` (ex.: `primitives.slint`, `semantic.slint`) ou evoluir `ui/theme.slint` mantendo `export global Theme` como contrato semântico; consolidar e remover duplicatas conforme §11.2.
- **Primitivas:** manter `ui/components/` (ou renomear para `ui/primitives/` na migração, se decidido) e **exportar publicamente** as primitivas normalizadas para consumo externo. Opções compatíveis:
  - adicionar `export { IconButton, PillButton, MediaButton, DesktopButton, InfoCard /* ou Surface */ } from "components/..."` (ou de um agregador `ui/primitives.slint`) em `ui/library.slint`, **preservando** os exports de janelas existentes; ou
  - expor um módulo agregador dedicado importável pelo catálogo.
- **`ui/library.slint`:** passa a ser o ponto de exportação de tokens/Theme + primitivas + janelas, sem quebrar consumidores atuais.
- **Rust:** `src/theme.rs` atualizado para o contrato semântico; `src/lib.rs` continua `slint::include_modules!()`; validar que o `build.rs`/bundle (`ui_loader`) inclui os novos arquivos de tokens/primitivas.

#### 11.4.3 Convenções do catálogo (leve/performático)

- Navegação simples (lista/abas lateral) que instancia apenas a seção ativa ou mantém previews estáticos pequenos.
- Stories como composições declarativas curtas dos componentes reais; sem timers/animações contínuas desnecessárias.
- Seletor de tema/preset ligado às propriedades do `Theme` (ou globals de modo) para alternar `light/dark` e direção de identidade.

### 11.5 Direções iniciais de identidade (comparar no catálogo)

Hipóteses a materializar como presets/temas no catálogo (D3). Valores exatos a ajustar na Fase 4 após validação visual (inclusive sobre desktop/Wayland).

| Direção | Intenção | Paleta / superfície | Tipografia / densidade | Raio / forma | Observações |
|---|---|---|---|---|---|
| **A — Baseline refinada** | Evoluir o visual atual com consistência | Neutros escuros/claros equilibrados; accent azul próximo ao atual (recalibrado para contraste), light/dark equivalentes | Escala curta e legível; densidade atual levemente regularizada por spacing tokens | Radius moderado e consistente (escala unificada, menos variação ad-hoc) | Menor risco de ruptura; foca em tokenização e contraste |
| **B — Neutro minimal** | Minimalismo mais “silencioso” | Neutros mais suaves/menos saturados; superfícies discretas; accent contido (dessaturado ou ajustável) com uso parcimonioso | Ênfase em hierarquia tipográfica e respiro (spacing) | Radius coerente (ligeiramente mais suave ou mais reto) aplicado de forma uniforme | Maior diferenciação; validar legibilidade/contraste e presença do accent no shell |

**Procedimento:** implementar ambas como temas selecionáveis no catálogo, lado a lado com o baseline tokenizado, e registrar a decisão (valores finais/preset default) na Fase 4.

### 11.6 Checklist de saída da Fase 0 (concluída neste detalhamento)

- [x] Inventário concreto de tokens/literais por primitiva alvo (§11.1).
- [x] Mapeamento `token atual → semântico proposto` e lacunas/primitives/ponte Rust (§11.2).
- [x] Lista final de primitivas, variantes, estados e stories do MVP (§11.3).
- [x] Esqueleto de arquivos/crate do catálogo e evolução de exports em `beca-ui` (§11.4).
- [x] Direções iniciais de identidade a comparar (§11.5).

**Próxima etapa:** implementação da **Fase 1 (tokens/foundations)** seguida da **Fase 2 (normalização das primitivas)** e **Fase 3 (catálogo MVP)** conforme §8, adotando o mapeamento M3 (§12), validando preservação das janelas existentes e registrando desvios/decisões neste documento.

### Progresso de implementação (12 nov 2026)

- **Fase 1 (parcial):** `ui/theme.slint` com tokens legados remapeados para baseline M3 roxo (dark, #6750A4/#D0BCFF) + roles/shape/state M3 completos expostos. `src/theme.rs` (`ThemeTokens`/defaults, `VoiceTheme` default) alinhado aos valores M3 preservando API/compat (`from_voice_theme`, `apply_typed`/`apply_interpreter`, fallbacks de cor inválida).
- **Fase 2 (parcial, MVP):** primitivas normalizadas para tokens/estados M3 mantendo APIs: `desktop_button.slint`, `icon_button.slint`, `info_card.slint` (novo `outlined`), `pill_button.slint`, `media_button.slint` (novo `on-accent`). `WaveformView`/`OscilloscopeView` permanecem adiadas.
- **Fase 3 (MVP inicial):** criado `crates/beca-ui-catalog` (bin `beca-ui-catalog`, workspace member) com `ui/catalog.slint` consumindo `beca-ui/ui/library.slint`; mostra foundations (amostras de roles/surfaces) e stories de estados das primitivas MVP + toggle de `dark-mode`.
- **Verificação:** `cargo check -p beca-ui -p beca-ui-catalog` OK; `cargo test -p beca-ui --lib` → 18 passed. Avisos Slint pré-existentes (exports de componentes não-Window) permanecem.
- **Desvios/pendências:** toggle do catálogo altera apenas `Theme.dark-mode` (sem troca completa de esquema light por preset derivado ainda); ícones no catálogo usam glyphs seguros (Material Symbols/vetores a integrar como asset/fonte); presets light/dark M3 completos, exports públicos adicionais e migração de padrões (bar/voice/launcher) ficam para iteração seguinte. Nenhuma stack reiniciada.

## 12. Adoção Material 3 e protótipo no Figma

> Decisão tomada em 12 nov 2026: aplicar **Material 3** com **baseline roxo padrão (seed #6750A4)** como base de identidade/tokens, usando o M3 Design Kit como referência de componentes, e adaptar ao contexto do shell (Wayland/bar/launcher/overlays) durante a implementação.

### 12.1 Decisão

- **Sistema:** Material 3 (roles de cor, type scale, shape scale, state layers, elevação/surface containers) em vez do sistema semântico custom inicial das seções 4/11.2.
- **Seed/baseline:** roxo M3 padrão **#6750A4** (light) / **#D0BCFF** (dark, primary) com esquema tonal padrão (secondary `#625B71`/`#CCC2DC`, tertiary `#7D5260`/`#EFB8C8`, error, neutros/superfícies M3).
- **Adaptação prevista (beca/shell):** ajustar superfícies para uso desktop/overlay (mais neutras/menos “tintadas” quando necessário, contraste sobre wallpaper), densidade compacta, shape contido onde fizer sentido, e mapeamento de componentes ao domínio (bar, launcher, voice, painéis).

### 12.2 Protótipo no Figma (fonte de referência visual)

Arquivo: **“Plugins for Dev Mode Playground (Copy)”**, página **“Beca”** (estrutura construída via `figma-mcp-rust`).

- **`Beca M3 Tokens`** (`VariableCollectionId:507:205`, modo `Value`, prefixos `light/` e `dark/` por limitação de 1 modo no plano gratuito): roles de cor M3 (primary/onPrimary/containers, secondary, tertiary, error, background/surface/surface-variant + surface-container lowest→highest, on-*, outline/outline-variant, inverse-*, scrim/shadow), **shape** (none/xs/sm/md/lg/xl/full = 0/4/8/12/16/28/9999) e **state layers** (hover/focus/pressed/dragged, disabled content/container).
- **Text styles M3:** `BecaM3/Display Small`, `Headline Small`, `Title Large/Medium`, `Body Large/Medium`, `Label Large/Medium`.
- **Seções:**
  - **01 — Foundations:** cores (roles M3 dark/light), tipografia, shape & spacing (grid 4dp), state & motion, ícones vetoriais/glyphs seguros (referência).
  - **02 — Primitives:** IconButton (standard/filled/tonal/outlined), Button (Filled/Tonal/Outlined/Text), Chip (Assist/Filter/Selected), FAB (small/primary/tertiary/surface), Card/Surface (Elevated/Filled/Outlined) — M3 roxo.
  - **03 — Identity:** M3 baseline roxo vs adaptado ao beca.
  - **04 — M3 Components (ref, kit):** Switch, Tabs (Primary), Top App Bar, Navigation Bar, Search, Text Field (Filled/Outlined), Slider, Checkbox & Radio, List, Navigation Rail, Dialog & Snackbar, Progress & Segmented, Menu & Tooltip.
  - **05 — Patterns (Beca):** Configuração (rail + aparência/modo/seed/toggles), Voice (orb idle/ouvindo/processando/falando + HUD), Launcher (search + resultados + hints), Barra (top/bottom, widgets), Painel (quick settings + sliders + notificações).

### 12.3 Mapeamento M3 → contrato do beca (guia para implementação)

| Conceito atual/proposto (beca) | Role/token M3 sugerido | Notas |
|---|---|---|
| `bg` / background app | `surface` / `background` (ou `surface-container-lowest/low`) | para shell/overlay, avaliar superfícies neutras escuras |
| `surface`, `card-bg`, `surface-alt` | `surface-container*`, `surface-container-high/highest`, `surface-variant` | consolidar duplicatas em papéis M3 |
| `text-primary` / `text-strong` | `on-surface` | ênfase via tipografia/peso, não novo token de cor |
| `text-secondary` | `on-surface-variant` |  |
| `text-on-accent` | `on-primary` / `on-primary-container` / `on-secondary-container` | conforme container usado |
| `accent`, `accent-hover/pressed` | `primary` (+ state layer overlay) / `primary-container` | estados por state layer/opacidade M3, não só cor fixa |
| `border`, `card-border` | `outline-variant` / `outline` |  |
| `focus-ring` | `primary` (ou `secondary`) como indicador de foco | garantir visibilidade |
| `status-success/danger/warning/recording` | `tertiary`/custom-success, `error`, `tertiary`/warning dedicado, `error` ou `primary` | M3 não define success/warning; manter extensões semânticas mínimas |
| `radius` | `shape/*` (xs/sm/md/lg/xl/full) | mapear escala do beca → shape M3 |
| tipografia | `type` M3 (display/headline/title/body/label) | preferir body/label/title para densidade desktop |
| `disabled` | `on-surface` com `state/disabled-content` (38%) e container 12% | padrão M3 |

**Implicações:**
- Evoluir `Theme`/`ThemeTokens` e `theme.rs` para expor (ou mapear para) roles M3, mantendo aliases de compatibilidade com config existente quando viável.
- Considerar **`slint-ui/material-components`** como base/referência de implementação; decidir entre adotar componentes da lib vs primitivas próprias tokenizadas em M3.
- Ícones: preferir **Material Symbols** (ou vetores/SVG) em vez de emojis; no Slint, alinhar `icon-font-family`/assets.

### 12.4 Ajustes em aberto (a validar no catálogo/Figma)

- Neutralização das superfícies para shell (reduzir tint/elevação visual onde conflitar com wallpaper/transparência).
- Densidade e shape finais para bar/launcher/overlays (compactação vs M3 mobile).
- Mapeamento final de variantes padrão: Button (Filled/Tonal), Pill/Chip (Assist/Filter/Segmented), Media (FAB vs IconButton), Card (Filled/Outlined).
- State layers e foco explícitos em todos os estados interativos; contraste AA onde aplicável.
- Páginas/estados adicionais: config (Voice/Bar/Launcher), launcher vazio, voice (erro/permissão/transcrição ao vivo), notificações/ações.

### 12.5 Decisões registradas (adendo)

| # | Decisão | Status |
|---|---|---|
| D4 | **Design system:** adotar **Material 3 baseline roxo (#6750A4)** como referência de tokens/componentes/padrões, com adaptação ao contexto do beca. | Confirmado |
| D5 | **Artefato de design:** Figma (página Beca) como protótipo vivo de foundations/componentes/padrões M3; catálogo Slint (`beca-ui-catalog`) permanece a fonte de verdade alinhada ao código. | Confirmado |
