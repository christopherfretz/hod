# A planted bug for `cargo xtask fuzz --mix` tests: the first two
# parameters of every function named callee_* that is not variadic and
# has no sret pointer among them change places, in definitions,
# declarations and calls alike. A module stays consistent with itself (its
# definitions name the two parameters, %arg0 and %arg1, and number the
# other values from %0), so only an object of another compiler sees it.
function renumber(s,    out, v) {
  out = ""
  while (match(s, /%[0-9]+/)) {
    v = substr(s, RSTART + 1, RLENGTH - 1) + 0
    out = out substr(s, 1, RSTART - 1) (v < 2 ? "%arg" v : "%" (v - 2))
    s = substr(s, RSTART + RLENGTH)
  }
  return out s
}
function label(s,    v) {
  if (match(s, /^[0-9]+:/)) {
    v = substr(s, 1, RLENGTH - 1) + 0
    return (v - 2) substr(s, RLENGTH)
  }
  return s
}
{
  line = $0
  if (inside) {
    if (line == "}") inside = 0
    else line = label(renumber(line))
  }
  out = ""; rest = line; swapped = 0
  while ((at = index(rest, "@callee_")) > 0) {
    pre = substr(rest, 1, at - 1); out = out pre; rest = substr(rest, at)
    open = index(rest, "(")
    out = out substr(rest, 1, open); rest = substr(rest, open + 1)
    depth = 0; n = 0; start = 1; len = length(rest); stop = 0
    for (i = 1; i <= len; i++) {
      c = substr(rest, i, 1)
      if (c == "(" || c == "{" || c == "[" || c == "<") depth++
      else if (c == "}" || c == "]" || c == ">") depth--
      else if (c == ")") { if (depth == 0) { stop = i; break } depth-- }
      else if (c == "," && depth == 0) { item[++n] = substr(rest, start, i - start); start = i + 1 }
    }
    if (stop == 0) { out = out rest; rest = ""; break }
    item[++n] = substr(rest, start, stop - start)
    ok = n >= 2 && item[1] !~ /sret/ && item[2] !~ /sret/ && pre !~ /\.\.\.\)/
    for (k = 1; k <= n; k++) if (item[k] ~ /\.\.\./) ok = 0
    if (ok) {
      a = item[1]; b = item[2]; sub(/^ /, "", b); item[1] = b; item[2] = " " a
      swapped = 1
    }
    list = item[1]; for (k = 2; k <= n; k++) list = list "," item[k]
    out = out list; rest = substr(rest, stop)
  }
  line = out rest
  if (swapped && line ~ /^define /) { line = renumber(line); inside = 1 }
  print line
}
