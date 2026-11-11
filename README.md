# Ardon Studio

One program with two complementary faces on one **Ardon-R2** session, so
nobody has to install two applications. A switch at the top right changes face.

- **Pro**, the script environment, as RStudio is for R: Source, Console,
  Environment and Plots panes.
- **Sakha**, menu-driven statistics in the spirit of Wizard 2 and Wizard Pro:
  the data comes first, you choose from menus, and you never type code.

Ardon Studio contains no Ardon-R2 code. It starts `r2 --ide` and talks to it over
stdin/stdout, one JSON object per line ([docs/IDE_PROTOCOL.md](docs/IDE_PROTOCOL.md)).
R2 does every statistic, printed value and plot; Ardon Studio shows them. Both faces
share one session, so data loaded in one is there in the other.

## Pro

| Pane | What |
|---|---|
| Source | Tabs of scripts and data viewers. Syntax colours, undo, `Ctrl+Enter` runs the line or selection, `Ctrl+Shift+Enter` runs all |
| Console | `R2>` prompt, `+` continuation, history, copy and paste. `Esc` stops running code |
| Environment | Variables as R2 describes them, and History. Click a data frame to view it; `[Sakha]` opens it in Sakha |
| Plots / Files | Plot history, save as SVG or PNG, copy as an image. The working folder: click a script to open it, a CSV to load it |

The dividers between panes drag. `View(x)` opens the data viewer, one page at a
time, so large frames open at once.

## Sakha

- **Overview**: every variable as a card with a small chart, in R2's numbers.
  Click one for its detail and a reading in plain words.
- **Data**: the data frame as a sheet.
- **Results**: each analysis as a card with R2's output, its plot and a line in
  plain words, such as "p = 0.31: no clear evidence that the group averages differ."

| Menu | Analyses |
|---|---|
| Summarize | All variables, Frequency table, Summary by group |
| Compare | One-sample, Two-group (Welch) and Paired t-tests, One-way ANOVA |
| Relate | Correlation, Chi-square, Linear regression, Logistic regression |
| Plot | Histogram, Density curve with 95% CI, Box plot by group, Scatter plot, Bar chart |

Each item opens a dialog that offers only the variables its role can take.
Everything done is recorded as a **session script**: read it beside the results,
export it as `.r2`, or open it in Pro. `Run in Pro` sends the code on
screen to the Console.

Sakha's numbers are checked against GNU R 4.5.3 (`crates/sakha-core/tests/real_r2.rs`).
Two R2 gaps found that way shape the menu, and a test fails when R2 changes them:

- `table(a, b)` keeps only `a`, so Chi-square builds its table with `matrix()`.
- `wilcox.test` differs from GNU R, so Mann-Whitney is not on the menu yet.

## Saving your work

| File | What it is | Where it lives |
|---|---|---|
| `.r2proj` | Marks a **folder** as a project, as `.Rproj` does. Remembers open scripts, pane layout, the face in front and the open analysis | one in the project's folder |
| `.sakha` | One menu-driven **analysis**, like a jamovi file: a link to the data, the steps as R2 code, each result as R2 printed and drew it, and where you were | anywhere, alone or in a project; as many as you like |

The button at the top right creates, opens and closes projects. Opening one starts
a fresh R2 session whose working folder is the project's folder, in both faces.
With no project, R2 works in Documents, or beside a `.sakha` file you open.

In Sakha, use **Save** in the header, File ▸ Save analysis or Ctrl+S. The header
shows the file's name, with `*` while there are unsaved changes, and quitting warns
you. Opening the file puts you back where you were:

- R2's memory is rebuilt by running the load and the data-changing steps again.
- Results are shown as saved, so the numbers you reported do not change quietly. If
  the data file has changed, Sakha says so and offers **Run all again**. If it is
  missing, the saved results are still there to read.
- Unsaved work is backed up every 30 seconds, and offered back after a crash.

**From Sakha to scripts:** build a workflow from the menus, then **Edit in Pro**
opens its session script as an ordinary script to fine-tune. In Pro, **Import
CSV** loads a file into R2 and opens it in a data tab. A file inside the working
folder is loaded by a relative path, so scripts keep working when the folder moves.

**Density curve with 95% CI:** R2's `density()` with a pointwise band, `f(x) +/- 1.96 se`, written as ordinary R2 code to read and fine-tune. It agrees with GNU R to about 0.01%. Histograms carry no interval.

## Data menu

Laid out on the lines of Rcmdr's, in both faces. It offers only what R2 honours.

| Item | What it does |
|---|---|
| Import text file | shows the file's first lines; asks the dataset's name, whether the first line holds names, and the separator (comma, tab, semicolon, space, guessed from the file) |
| Import Parquet file | `read.parquet` |
| Load R2 data file (`.r2s`) | `load()`, with a warning: R2's `save()` and `load()` currently lose factor columns |
| R2 datasets | one searchable list, asked of R2 itself, for however many there are. `airquality` waits until R2's matches R's (30 rows against 153) |
| Select dataset | the data frames in the session, wherever they were loaded |
| Variable names and types (Sakha) | rename a column; number or category. Values are never edited here |

In Sakha, the **Dataset** button in the header does the same as Select dataset. Data
loaded in Pro is simply there: if it is the only data frame, switching to
Sakha opens it. Column changes are R2 code in the session script, and run again on
reopening a `.sakha`.

Not offered, because R2 cannot do them yet: missing-value codes (`read.csv` ignores
`na.strings`), decimal commas (ignores `dec`), Excel, SPSS, Stata, SAS, renaming
factor levels (`levels<-`).

## Crates

| crate | what |
|---|---|
| `crates/r2sakha` | the app (`ArdonStudio.exe`) |
| `crates/sakha-core` | headless logic: analyses, variable profiles, session script, layout |
| `crates/sakha-engine` | the link to R2: starts `r2 --ide`, sends requests, reads output and replies |
| `crates/r2-ui` | Ardon Studio's own copy of Ardon-R2's GUI framework (winit + wgpu + fontdue) |
| `crates/r2-editor` | script editor core (buffer, undo, highlighting, Run statement) |

## Install

Download the file for your system from the
[latest release](https://github.com/devendratandle/R2Sakha/releases/latest). Each
one includes Ardon-R2 (`r2` with `--ide`), so Ardon Studio runs at once.

| System | File | How |
|---|---|---|
| Windows 10/11 (x64) | `ArdonStudio-Setup-<version>.exe` | Run it. It installs for your user account, with no admin rights, and can associate `.sakha` and `.r2proj` files with Ardon Studio. |
| Ubuntu 22.04+, Debian 12+ (x86_64) | `ardon-studio_<version>_amd64.deb` | `sudo apt install ./ardon-studio_<version>_amd64.deb` |
| Other Linux (x86_64, glibc 2.35+) | `ArdonStudio-<version>-linux-x86_64.tar.gz` | Unpack it and run `./install.sh` (for you only) or `sudo ./install.sh` (for everyone). |
| macOS 11+ (Apple silicon) | `ArdonStudio-<version>-macos-arm64.dmg` | Open it and drag Ardon Studio onto Applications. The app is not signed by Apple yet; `README-FIRST.txt` in the disk image says how to open it the first time. |

The Linux and macOS builds are new in 0.0.2 and less tested than the Windows one.

The files are built by `.github/workflows/release.yml` when a version tag is pushed.
To build the Windows installer by hand: `cargo build --release -p r2sakha`, copy an `r2.exe` built
with `--ide` to `installer\bundle\`, then compile `installer\ArdonStudio.iss` with
Inno Setup 6.

## Engine

Ardon Studio needs an `r2` that supports `--ide` (Ardon-R2 with handoff 7). It uses
the first it finds:

1. `ARDON_STUDIO_R2`: the full path of `r2.exe`
2. `r2.exe` next to `ArdonStudio.exe`
3. `r2.exe` on `PATH`
4. `C:\Program Files\Ardon-R2\r2.exe`

An older `r2` without `--ide` is reported in the Console.

While Ardon Studio runs, Windows keeps its `r2.exe` locked. **Do not point it at
Ardon-R2's `target\release\r2.exe` while building Ardon-R2**: copy the exe next
to `ArdonStudio.exe` instead, and copy it again after each new R2 build.

## Build and test

```powershell
cargo build --release -p r2sakha
Copy-Item <Ardon-R2>\target\release\r2.exe target\release\
.\target\release\ArdonStudio.exe

$env:ARDON_STUDIO_R2 = "<a copy of r2.exe>"
cargo test --workspace          # without ARDON_STUDIO_R2 the engine tests skip
```

`--features drive` adds a scripted-input test driver (`crates/r2sakha/src/drive.rs`)
for checking the real window. It is never part of a release.

License: AGPL-3.0.
