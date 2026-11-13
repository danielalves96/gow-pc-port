<div align="center">

<img src="https://i.ibb.co/FLBHkcHL/god-of-war-native-pc-port-banner.png" alt="God of War: port nativo para PC" width="100%">

[English](README.md) · **Português (Brasil)**

*God of War* (PlayStation 2, NTSC-U `SCUS-97399`) **recompilado estaticamente** para C++ nativo - para Windows e, experimentalmente, Linux.

[![Recompilação estática](https://img.shields.io/badge/recompila%C3%A7%C3%A3o%20est%C3%A1tica-n%C3%A3o%20%C3%A9%20emulador-2ea44f)](#não-é-um-emulador)
[![Plataforma](https://img.shields.io/badge/plataforma-Windows%20%7C%20Linux%20x64-0078D6)](#suporte-por-plataforma)
[![Linguagem](https://img.shields.io/badge/C%2B%2B-20%20%2B%20Rust-00599C)](docs/dev/ARCHITECTURE.md)
[![Renderer](https://img.shields.io/badge/renderer-OpenGL%204.6-5586A4)](docs/dev/RENDERING.md)
[![Licença](https://img.shields.io/badge/licen%C3%A7a-GPL--3.0-blue)](LICENSE)
[![Discord](https://img.shields.io/badge/Discord-Join%20the%20community-5865F2?logo=discord&logoColor=white)](https://discord.gg/QQmjMx7hz)

</div>

---

> [!TIP]
> **Download:** o programa está na página de [**Releases**](https://github.com/danielalves96/gow-pc-port/releases). Baixe a
> [**última release**](https://github.com/danielalves96/gow-pc-port/releases/latest): no Windows o instalador
> (`..._x64-setup.exe`) ou o zip portátil, no Linux o `.deb` ou o AppImage. O botão verde **Code** (e o "Source code"
> de uma release) só entrega o código-fonte, sem instalador: serve para quem quer compilar por conta própria. Você também
> precisa da sua própria cópia do jogo, veja [Instalação](#instalação).

> [!IMPORTANT]
> **Isto não é um emulador.** É **recompilação estática (recomp)**: o executável MIPS do jogo é traduzido para C++
> antecipadamente e compilado como um programa nativo. **Não há interpretador MIPS, hardware de PS2 emulado nem BIOS**
> em tempo de execução. [`docs/dev/ARCHITECTURE.md`](docs/dev/ARCHITECTURE.md) mostra exatamente onde fica a fronteira.

> [!IMPORTANT]
> **Este repositório não distribui nenhum dado do jogo.** Nada de ISO, `SCUS_973.99`, módulos IRX, arquivos `.PAK` ou
> código gerado a partir deles. Você precisa da sua própria cópia legal de *God of War* para PlayStation 2 - a versão
> **americana**, `SCUS-97399`. O instalador compila tudo na sua máquina a partir do seu disco; nada derivado do jogo
> sai dela.

## Resumo

**Recompilação estática, não emulação.** O código MIPS do jogo é **traduzido para C++** por um recompilador escrito em
Rust e depois compilado como um programa de PC comum. O hardware do PlayStation 2 ao redor dele - sintetizador gráfico,
unidade vetorial, processador de I/O, som, controle, cartão de memória - é implementado de forma nativa. Nada
interpreta o código do jogo em tempo de execução.

O resultado é o jogo na lógica original de 60 Hz (ou num limite de 120 FPS, o máximo, na mesma velocidade), com
renderizador OpenGL de 1× a 8× de resolução interna, vídeos
nativos com som, um launcher com ajustes, teclado e mouse, menu de cheats, mira travada, widescreen, packs de texturas
personalizadas e RetroAchievements.

## Não é um emulador

| | Um emulador (ex.: PCSX2) | Este port |
|---|---|---|
| Como o jogo roda | Executa o binário MIPS original instrução por instrução | **Traduz para C++ uma vez** e compila isso em `gow-2005` |
| Hardware do PS2 | Emulado: CPU, GS, VU, IOP, DMA, timers… | **Serviços nativos**: GS em OpenGL, microcódigo de VU1 compilado antecipadamente, módulos IRX do jogo compilados para C++ na instalação, a partir do disco (sem abrir o jogo) |
| BIOS | Exige um dump da BIOS do PS2 | **Nenhuma.** Os serviços de kernel e biblioteca são implementados no runtime |
| Seu disco | Montado/emulado a cada execução | Lido uma vez na instalação; depois os arquivos extraídos são lidos por código nativo de disco |
| O que você executa | Um aplicativo de console dentro de uma máquina virtual | Um executável nativo comum, `gow-2005` |

A consequência que importa: o código do próprio jogo e o hardware ao redor dele são **C++ compilado**, não algo
reproduzido quadro a quadro. Truques de precisão de emulação (hacks de frame, de ciclo, patches por jogo) não são como
este port funciona; o que ele tem é o comportamento do jogo vindo do seu próprio código recompilado, mais substituições
nativas das bibliotecas que ele chama.

## Recursos

- **Renderizador nativo** - OpenGL 4.6, render targets na GPU, resolução interna de 0,25× a 8× (abaixo de 1× para GPUs integradas), upscaling bicúbico (com
  nitidez), bilinear ou pixel, FXAA, modo progressivo 480p, ajuste de brilho/contraste/gama/saturação.
- **Widescreen (Hor+)** - 16:9, 16:10, 21:9 (experimental) e 32:9 (muito experimental: HUD e menus esticados) pelo patch do PCSX2, com chave; desligado mostra o 4:3 original.
- **Vídeos** - MPEG-2 decodificado nativamente, com som, incluindo as versões widescreen do próprio jogo.
- **Controles** - teclado e mouse remapeáveis, qualquer controle, layout PS2 ou moderno, troca por botão, vibração no
  Windows e no Linux.
- **Qualidade de vida** - menu de cheats (**F2**, nove cheats), mira travada (**Alt**), combate ágil, informações na tela com FPS, GPU, CPU e RAM (cada uma opcional),
  tela cheia.
- **Saves** - cartão `Mcd001.ps2` compatível com o PCSX2, nos dois sentidos. Uma cópia datada do cartão é guardada a cada save num altar. A aba Saves do launcher mostra cada save com o altar, a miniatura, a dificuldade e o tempo de jogo, carrega um deles mesmo com o jogo aberto e restaura uma cópia.
- **Texturas personalizadas** - packs de substituição do PCSX2, com o pack da comunidade baixável pelo launcher.
- **Steam e atalhos**: o launcher pode abrir o jogo direto, sem a janela dele, e fechar junto com o jogo, então dá para adicioná-lo à Steam como jogo não Steam ([`docs/LAUNCHER.md`](docs/LAUNCHER.md#starting-the-game-without-the-launcher)).
- **RetroAchievements** - login, notificações no jogo e modo hardcore que desativa os cheats.
- **Launcher** - nove abas, assistente de instalação, otimização em segundo plano, inglês e português do Brasil.

## Suporte por plataforma

| | Windows | Linux |
|---|---|---|
| Instalador | `install.cmd` / `install.ps1` (14 opções) | `install.sh` |
| Launcher | `God Of War - PC Launcher_<versão>_x64-setup.exe` ou o `..._x64-portable.zip`, com o assistente de instalação | `.deb` / AppImage, com o assistente de instalação (ele usa o `install.sh`) |
| Situação | **Alvo principal** | **Experimental** |

> [!WARNING]
> **O Linux é experimental.** O caminho Linux existe no repositório - `install.sh`, o caminho de renderização
> GLFW/GL, `run_gow.sh`, o empacotamento `.deb`/AppImage - mas não está validado no mesmo nível: nenhum job de CI
> compila ou testa o runtime em plataforma alguma, o assistente do launcher usa o `install.sh` no Linux mas nunca rodou
> em uma máquina Linux de verdade e builds recentes no Linux não foram medidos. Trate
> como preparado, não como suportado. [`docs/STATUS.md`](docs/STATUS.md) tem os detalhes, área por área.

## Índice

- [Resumo](#resumo)
- [Não é um emulador](#não-é-um-emulador)
- [Recursos](#recursos)
- [Suporte por plataforma](#suporte-por-plataforma)
- [O que você precisa](#o-que-você-precisa)
- [Instalação](#instalação)
- [Jogando](#jogando)
- [Desempenho](#desempenho)
- [Estrutura do repositório](#estrutura-do-repositório)
- [Como funciona](#como-funciona)
- [Por que a build é lenta e grande, e por que o PS2 é difícil](#por-que-a-build-é-lenta-e-grande-e-por-que-o-ps2-é-difícil)
- [Documentação](#documentação)
- [Contribuindo](#contribuindo)
- [Créditos](#créditos)
- [Licença e aspectos legais](#licença-e-aspectos-legais)

## O que você precisa

- **Sua própria ISO de *God of War* (EUA, `SCUS-97399`)**, cerca de 8 GB. Um disco modificado sem mudar o código do jogo
  (tradução, redublagem, mod de dados) também funciona, e um patch `.xdelta` pode ser aplicado à original no assistente.
  Uma imagem `.chd` (do `chdman`) também serve: o assistente a converte para uma ISO ao lado dela.
- **~20 GB livres** durante a instalação (sua ISO, o disco extraído e a build); ficam uns 11 GB depois de apagar a ISO.
- **CPU x86 de 64 bits com SSE4.1** (Intel Core 2 "Penryn", 2008, ou mais nova; AMD "Bulldozer", 2011, ou mais nova; o AMD Phenom II não tem), 4 núcleos ou mais recomendado. AVX2 **não** é necessário.
- **GPU com OpenGL 4.6** (qualquer NVIDIA/AMD recente, ou iGPU recente da Intel/AMD).
- **8 GB de RAM** (16 GB para compilar com folga).
- **Windows 10/11 x64** ou um Linux recente (Arch/CachyOS, Ubuntu 24.04+, Fedora 40+) - Linux é experimental.
- **Uns 10–20 minutos** para a primeira compilação no Windows (10 num Intel Core i5-12400F de 6 núcleos; mais em 4
  núcleos ou CPUs mais antigas). No Linux, que ainda compila com g++, pode levar de 30 a 90 minutos.

## Instalação

**Windows - o launcher.** Baixe o instalador na página de Releases, execute e deixe o assistente fazer o trabalho: ele
encontra e verifica a sua ISO, baixa um compilador C++ portátil (sem Visual Studio desde a 0.3.0), extrai o disco, recompila o jogo e
compila tudo. Depois é só apertar **PLAY**.

> **O Windows pode avisar sobre um "editor desconhecido".** O instalador do launcher ainda não tem assinatura de
> código, então o SmartScreen mostra "O Windows protegeu o computador". Clique em **Mais informações** e depois em
> **Executar assim mesmo**. O instalador não contém arquivos do jogo e o código-fonte dele está neste repositório.
>
> **Prefere não rodar um instalador?** O mesmo release tem um **zip portátil** (`..._x64-portable.zip`): descompacte,
> abra o `God Of War - PC Launcher.exe` e o assistente faz o resto. Desbloqueie o zip antes de descompactar (botão
> direito, Propriedades, Desbloquear), senão o Windows mostra o mesmo aviso para o `.exe`. Ele precisa do WebView2,
> que o Windows 11 e o Windows 10 atualizado já têm. Todo release também lista o SHA-256 dos arquivos em
> `SHA256SUMS-*.txt`.

**Linux - o script (experimental).**

```bash
./install.sh --install-deps "/caminho/God of War.iso"   # ou coloque a ISO em game/ e rode ./install.sh
./run_gow.sh                                            # jogar
```

Antes de começar, o script confere CMake, Ninja, um compilador C++, Git e os arquivos de desenvolvimento de GL, X11,
Wayland e ALSA, além de espaço em disco (20 GB) e RAM (no mínimo 4 GB, 8 GB recomendados). No Arch, Debian/Ubuntu e
Fedora (e derivadas) o `--install-deps` instala o que faltar usando `sudo`; sem a opção, ele pergunta quando está
em um terminal e, caso contrário, mostra o comando. Em qualquer outra distribuição ele lista o que instalar. O Rust
não é necessário quando o kit do launcher traz as ferramentas. O assistente do launcher faz a mesma verificação no
Linux, mas o comando de instalação dos pacotes você roda por conta própria, porque ele pede a sua senha.

Os dois instaladores aceitam opções (ferramentas pré-compiladas, pasta de destino, número de jobs, otimização) - todas
em [`docs/INSTALL.md`](docs/INSTALL.md).

> A primeira compilação compila cerca de 6.400 arquivos C++ gerados. Atualizações depois disso recompilam só o que
> mudou.

## Jogando

Teclado e mouse funcionam de imediato e são totalmente remapeáveis no launcher:

| | | | |
|---|---|---|---|
| Mover | **W A S D** | Ataque leve / pesado | **Clique esquerdo / direito** |
| Pular | **Espaço** | Agarrar | **F** |
| Bloquear | **Q** | Ação (R1) | **E** |
| Magia (L2) | **Shift esquerdo** | Interagir (R2) | **Ctrl** parado |
| Fúria dos Deuses | **V** | Lâmina de Ártemis | **Clique do meio** |
| Magias 1–4 | **1 2 3 4** | D-pad / câmera | **Setas** / **I J K L** |
| Menu de cheats | **F2** | Tela cheia | **F11** |
| Mira travada | **Alt** (mouse troca o alvo) | Painel de desenvolvedor | **F1** |

Controles (DualShock, DualSense, Xbox, 8BitDo) funcionam direto; a vibração funciona no Windows. Os saves ficam em uma
imagem de cartão `Mcd001.ps2` ao lado do executável, **compatível com o PCSX2 nos dois sentidos**.

O resto - lista completa de cheats, regras da mira travada, combate ágil, cartão de memória, conquistas - está em
[`docs/PLAYING.md`](docs/PLAYING.md). As abas e opções do launcher estão em [`docs/LAUNCHER.md`](docs/LAUNCHER.md).

## Desempenho

O jogo roda a 60 FPS por padrão, a taxa do próprio PS2, sempre na velocidade real: quadros atrasados são descartados,
nunca desacelerados. O limite de 120 FPS do launcher roda a lógica do jogo nessa taxa, mais fluido mas nunca mais rápido;
em 60 a janela segue os quadros do próprio jogo, em 120 atualiza na taxa do monitor, no máximo 120. Nada roda acima de 120 FPS: além disso um PC fraco não fica mais
fluido, só mais quente.

- **Resolução interna 2× é o ponto ideal.** 3× ou mais é pesado e, em placas intermediárias, pode travar a apresentação.
- O lado da CPU é dominado pela **VU1** (unidade vetorial). Cada passe de otimização converte mais dela em código nativo:
  rode `install.cmd -Optimize` / `./install.sh --optimize` depois de jogar áreas novas, ou deixe a otimização em segundo
  plano do launcher terminar.
- As builds com clang (o padrão no Windows) já usam um perfil de velocidade (PGO) que vem no repositório e não contém nada
  do jogo. O **Otimizar agora** do launcher (`-Pgo on` / `--pgo on`) é um extra opcional que treina um perfil completo no seu
  PC e, nos testes, reduziu em cerca de 20 % a mais o tempo de VU1 e EE por quadro (ele abre o jogo sozinho por 4 minutos).
- **O perfil de economia de energia do notebook pode cortar a taxa pela metade**, porque reduz o clock da CPU de que a
  thread da VU1 precisa.

Números medidos e a ordem de ajuste estão em [`docs/PERFORMANCE.md`](docs/PERFORMANCE.md).

## Estrutura do repositório

```
.
├── install.sh / install.cmd / install.ps1   instaladores (Linux / Windows)
├── run_gow.sh                               jogar sem o launcher (Linux)
├── RustPortPS2/
│   ├── crates/                              Rust: ps2_recomp, ps2-tools, ps2-parity, ps2-devtools, …
│   ├── native/                              runtime C++: ps2xRuntime, ps2xIOP, ps2xTest, tools, tests
│   ├── games/scus-97399/                    configuração do recompilador, mapa de funções, manifestos de VU1
│   ├── third_party/rabbitizer/              desassemblador MIPS vendorizado (MIT)
│   └── docs/                                PARITY.md (o portão byte a byte)
├── gow-launcher/                            launcher (Rust gow-core + Tauri 2 + React)
├── scripts/                                 auxiliares de build e o verificador de scripts da CI
├── docs/                                    documentação do jogador (dev/ para contribuidores)
└── LICENSE                                  GPL-3.0
```

O código gerado, o disco extraído e a compilação ficam em `game/`, `build-gen/`, `build/` e `vu1_captures/` - todos
ignorados pelo Git.

## Como funciona

São dois programas diferentes, e eles nunca rodam ao mesmo tempo:

| | Recompilação | Partida |
|---|---|---|
| **Quando** | uma vez, durante a instalação | toda vez que você abre o jogo |
| **O que roda** | `ps2_recomp` (Rust), `ps2-tools`, CMake e Ninja | `gow-2005`, o jogo compilado |
| **Entrada** | seu `SCUS_973.99`, a configuração do recompilador (`gow-port.toml`, `funcmap.csv`) e o manifesto de VU1 | o C++ gerado acima, mais o disco extraído |
| **Saída** | ~6.400 arquivos C++ e os blocos nativos de VU1, ligados em `gow-2005` | quadros, som e saves - sem interpretador no caminho |

Em tempo de execução, o código do jogo é C++ comum: **não há interpretador MIPS nem emulador no caminho crítico**. Ao
redor dele existe um runtime que faz o papel do hardware do PS2 - o GS renderiza em OpenGL, a VU1 executa microcódigo
compilado antecipadamente, o IOP roda os próprios módulos IRX do jogo como código nativo, e som, controle e cartão de
memória são serviços do host.

O recompilador reproduz byte a byte a saída do [PS2Recomp](https://github.com/ran-j/PS2Recomp); o runtime C++ é construído
sobre o *God-Of-War-Native-Port* de Waffle565, com o backend OpenGL de GS vindo do fork SotC do PS2Recomp e o PCSX2 usado
como referência de hardware. Arquitetura, opções de build e testes estão em
[`docs/dev/`](docs/dev/README.md).

## Por que a build é lenta e grande, e por que o PS2 é difícil

Os números de [O que você precisa](#o-que-você-precisa) (10 a 20 minutos no Windows, ~20 GB) não são exagero. Eles vêm do que o
instalador faz e da máquina que está sendo portada.

**Por que a primeira build ainda leva minutos**

- O recompilador transforma cerca de 6.400 funções do jogo em cerca de 6.400 arquivos C++, aproximadamente **129 MB de
  código-fonte**. Cada instrução MIPS vira C++ explícito (acesso a registradores, acesso à memória, delay slots de
  desvio, operações multimídia de 128 bits), então poucos bytes de código de máquina viram muitas linhas. O compilador
  otimiza tudo isso em modo Release, do zero, uma vez. É o tamanho de uma aplicação grande, não de um projeto pequeno.
  Desde a 0.3.0 o Windows compila com clang, que leva uns 10 minutos onde o Visual Studio levava mais de uma hora no
  mesmo PC.
- A VU1 (a unidade vetorial) executa microcódigo que é gerado antecipadamente como blocos nativos e também compilado.
  Sem essa etapa o jogo interpretaria o código da VU1 e rodaria bem mais devagar.
- O paralelismo é limitado pela memória, não só pelos núcleos: os instaladores iniciam no máximo um processo de
  compilação por 2 GB de RAM e limitam o gerador de VU1 a 4 GB por padrão. Um PC com muitos núcleos e pouca RAM compila
  devagar de propósito, para não ficar sem memória.
- Mudar um header incluído pelo código gerado, ou qualquer `CMakeLists.txt`, recompila tudo (o header pré-compilado é
  regenerado). Atualizações posteriores recompilam só o que mudou.

**Por que pede ~20 GB**

O jogo final é pequeno perto do que a instalação precisa. Medido numa instalação 0.3.0: sua ISO (cerca de 8 GB), o
disco extraído (7,9 GB), a build com arquivos objeto, header pré-compilado, blocos de VU1 e perfil de velocidade
(1,3 GB), o compilador portátil (0,5 GB, no Windows) e o C++ gerado, as ferramentas e a cópia do jogo que você joga
(cerca de 0,4 GB). São uns 18 GB no pico e uns 11 GB depois de apagar a ISO; o requisito deixa uma pequena folga.

**Por que o PS2 é mais difícil de portar que a maioria dos consoles**

Algumas comparações em linhas gerais, não um ranking geral de dificuldade:

| | Alvo típico de recompilação | PlayStation 2 |
|---|---|---|
| Processadores que rodam código do jogo | Uma CPU principal | Três, com conjuntos de instruções diferentes: o Emotion Engine (MIPS R5900 com extensões multimídia próprias de 128 bits), duas unidades vetoriais (VU0, VU1) e o processador de E/S (uma CPU MIPS da classe do PS1 rodando módulos IRX) |
| Gráficos | Uma GPU controlada por lista de comandos ou biblioteca fixa | O Graphics Synthesizer, com 4 MiB de memória de vídeo embutida, alimentado por DMA pela CPU e pela VU1; não existe API gráfica e os jogos programam o hardware diretamente |
| Microcódigo | Nenhum, ou poucos compartilhados (por exemplo os do N64 da Nintendo) | Cada jogo traz seus próprios programas de VU1. Eles são enviados em tempo de execução, então não dá para achar todos lendo o executável: é preciso capturá-los jogando |
| Acesso ao hardware | Por um sistema operacional ou biblioteca | Os jogos falam direto com registradores, DMA e timers, então o runtime precisa reproduzir esse comportamento |
| Tamanho do código | Muitas vezes pequeno (cartuchos ou jogos iniciais em disco) | *God of War* tem cerca de 6.400 funções no executável principal, fora os módulos do IOP |

Por isso este port precisa de um recompilador, um GS nativo, um gerador de VU1, uma camada de IOP e muitos dados
capturados, e não só de um tradutor para uma CPU. É também por isso que o instalador não pode entregar um binário
pronto: ele conteria o código do jogo, que este repositório nunca distribui (veja
[Licença e aspectos legais](#licença-e-aspectos-legais)).

## Documentação

| | |
|---|---|
| [`docs/INSTALL.md`](docs/INSTALL.md) | Requisitos, assistente, os dois instaladores, todas as opções, o que é criado |
| [`docs/LAUNCHER.md`](docs/LAUNCHER.md) | O launcher, aba por aba, com padrões, caminhos e logs |
| [`docs/PLAYING.md`](docs/PLAYING.md) | Controles, cheats, saves, controle, conquistas |
| [`docs/PERFORMANCE.md`](docs/PERFORMANCE.md) | O que custa o quê, referências medidas, ajustes |
| [`docs/TROUBLESHOOTING.md`](docs/TROUBLESHOOTING.md) | Soluções por sintoma, logs, relatos de bug |
| [`docs/STATUS.md`](docs/STATUS.md) | O que funciona, o que não funciona, problemas abertos |
| [`docs/TEXTURES.md`](docs/TEXTURES.md) | Packs de textura: instalar e criar |
| [`docs/THIRD_PARTY.md`](docs/THIRD_PARTY.md) | Projetos de origem, bibliotecas, licenças |
| [`docs/dev/`](docs/dev/README.md) | Arquitetura, build, variáveis de ambiente, renderização, áudio, diagnóstico |

## Contribuindo

Contribuições são bem-vindas - relatos de bug com log, correções, documentação. Leia
[`docs/dev/README.md`](docs/dev/README.md) e [`docs/dev/BUILD.md`](docs/dev/BUILD.md) antes; o resumo:

- Rust compila as ferramentas, C++ roda o jogo. Arquivos gerados nunca são editados à mão.
- Mudanças em código herdado levam um comentário `// GOW-Port:`.
- `cargo test --workspace` precisa passar, `cargo clippy … -D warnings` precisa continuar limpo e o portão de paridade
  precisa continuar byte a byte idêntico.
- Nunca faça commit de nada derivado do jogo.

## Créditos

Este port existe pelo trabalho de outras pessoas, em primeiro lugar
[PS2Recomp](https://github.com/ran-j/PS2Recomp),
[a recompilação de God of War do KIexster](https://github.com/KIexster/god-of-war-recomp),
[o port nativo do Waffle565](https://github.com/Waffle565/God-Of-War-Native-Port) e o
[PCSX2](https://github.com/PCSX2/pcsx2). Inventário completo, com o que vem de onde e sob qual licença:
[`docs/THIRD_PARTY.md`](docs/THIRD_PARTY.md).

<table>
  <tr>
    <td>
      <a href="https://github.com/danielalves96"><img src="https://avatars.githubusercontent.com/u/62755605?s=120&v=4" width="120" alt="danielalves96"/><br/><b>danielalves96</b></a><br/>
      Recompilador, integração do runtime, recursos de PC, launcher, empacotamento
    </td>
  </tr>
</table>

## Licença e aspectos legais

O código deste repositório é **GPL-3.0** (veja [`LICENSE`](LICENSE)), como os projetos sobre os quais ele é construído.
Bibliotecas vendorizadas mantêm suas próprias licenças, listadas em [`docs/THIRD_PARTY.md`](docs/THIRD_PARTY.md).

*God of War* © Sony Interactive Entertainment / Santa Monica Studio. Este projeto não é afiliado nem endossado pela
Sony e não distribui conteúdo do jogo. **Tenha o jogo original.**
