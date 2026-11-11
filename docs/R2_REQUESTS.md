# What Ardon Studio needs from Ardon-R2

Ardon Studio found the gaps below while running R2 through `r2 --ide`. Each item gives what R2 does now and what GNU R does. Items are ordered by how much they block Ardon Studio. Ardon Studio works around some of them today; a fix lets it drop the workaround.

## For Gview (editable graphs)

1. **`eval` replies omit overlays.** After `plot(...)` and then `lines()`, `points()` or `abline()`, the reply's `plot` holds only the first drawing. Ardon Studio calls `save.plot()` afterwards to get the whole picture.
   *Want:* the reply's `plot` is the complete current plot, overlays included.
2. **Mark the parts of the SVG.** Gview lets you click the title or an axis name to edit it.
   *Want:*
   - `class="main"`, `class="sub"`, `class="xlab"` and `class="ylab"` on those `<text>` elements.
   - `class="plot-region"` on the plotting area, with its user coordinates as `data-usr="x1 x2 y1 y2"`.
3. **`barplot(..., horiz = TRUE)`** still draws vertical bars.
4. **`plot(density(x))`** fails, and `density()` returns only `x` and `y`. GNU R also returns `bw`, `n` and `call`, and has a `plot` method.
5. **`xlim` and `ylim`** are ignored by `plot()`, and `hist()` probably ignores them too (not tested).
6. **Ignored:**
   - `title(sub = ...)`
   - `text(x, y, labels)`
   - `par(cex.main = ...)`

   The argument `cex.main = ...` inside the call does work.

## For the Data tab (editing like jamovi)

7. **`d[1, 1] <- value`** errors. `d[2, "x"] <- 9` and `d$x[3] <- 4` work. Gview and the Data tab write each cell edit as R2 code, so they need every form of `[<-` on data frames.
8. **`rbind(d, data.frame(...))`** fails when `d` has factor columns, so a row cannot be added.
9. **`cut()`** returns character; GNU R returns a factor.
10. **`save()` and `load()`** lose factor columns.
11. **`read.csv`** ignores `na.strings` and `dec`.

## For the Tables tab (APA tables)

12. **Coefficient names for factor terms.** `names(coef(lm(mpg ~ wt + factor(cyl), data = mtcars)))` gives `x26`, `x28`. GNU R gives `factor(cyl)6`, `factor(cyl)8`.
13. **`cor.test`** has no `conf.int`, and calls its degrees of freedom `df` rather than `parameter` as GNU R does.
14. **`anova(lm(...))`** has no `Df` column, and `[["F value"]]` does not work.
15. **`dimnames()`** and **`nobs()`** are missing.

## Correctness (results differ from GNU R)

16. **`table(a, b)`** ignores `b`, so a two-way table is one-way. Sakha's chi-square builds the table with `matrix()` and `sum()` instead.
17. **`wilcox.test`** gives a different p-value from GNU R, so Mann-Whitney is off Sakha's menu.
18. **The built-in `airquality`** has 30 rows; GNU R's has 153.

## For Pro's console (R2's own console experience)

`r2-console` is the single source of truth for how R2's console behaves: the banner (`banner_lines`), the `R2>` and `+` prompts, and when a statement is complete (`ConsoleBuffer`, `is_incomplete`). `--ide` mode does not pass any of it on, so Ardon Studio keeps its own copy (`console.rs`) and writes its own banner. These items make `--ide` one more frontend of `r2-console`, as R2Gui is:

19. **Banner and prompts in `hello`.** Add `"banner": banner_lines(version)` and `"prompts": {"main": "R2>", "continue": "+"}`. Pro then prints R2's banner verbatim, followed by Ardon Studio's single hint line, as the comment on `banner_lines` allows.
20. **Console input decided by R2.** Already done, no change needed: `eval` replies `"incomplete": true` for an unfinished statement. Pro now sends every line to R2 and shows `+` when R2 says so. It no longer counts brackets itself.
    *Still differs:* an unclosed string (`s <- "abc`) is an error in `--ide` ("unterminated string"), where R and R2's own console would ask for more with `+`. It would be better reported as `incomplete` too.
21. **Features in `hello`.** Add `"features": ["banner", "console", ...]`, and raise `PROTOCOL` only for changes that break existing messages. Ardon Studio uses a feature only when R2 lists it. Otherwise it falls back to what it does today, or tells the user to update R2. This lets either program be updated without the other.

## Status, 2026-11-11

R2 0.4.1 (commit `1356074` on R2's `main`) has items 1–21, except as noted here. Ardon Studio's tests pass against it.

22. **Still open: `chisq.test(table(a, b))`.** `table(a, b)` is now two-way, but `chisq.test` tests it as one row of counts ("given probabilities", df = 5, p = 0.347 on the test data). GNU R tests independence (df = 2, p = 0.0498). `chisq.test(matrix(...))` is right, so Sakha keeps writing the matrix out.

## Found while building the Data tab's editing, 2026-11-11

23. **`subset(d, cond)` leaves factor columns unfiltered** and drops the row names. After `mtcars$cyl <- factor(mtcars$cyl)` and `mtcars <- subset(mtcars, mpg > 20)`, `nrow` is 14 but `cyl` still has 32 values, so the data frame is broken. GNU R filters every column and keeps the row names. Ardon Studio writes `d[which(with(d, cond)), ]` instead, which R2 gets right.
24. **`replace()` is missing** (`object 'replace' not found`).
25. **`cut()` labels say `-inf` and `inf`**: `(-inf,1.5]`. GNU R writes `(-Inf,1.5]`.

## Checks

For each item:
- Add a test to R2 that compares the result with GNU R's.
- For items 1 and 2, check the `--ide` JSON reply itself, because Ardon Studio reads that reply.
