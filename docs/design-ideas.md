# Design ideas — UNSTABLE

> Aspirational, half-baked thoughts about NETCONF/YANG protocol & language
> design and a possible project milestone. **Not a commitment, not reviewed,
> not planned.** Anything here may be edited or deleted freely.
>
> Promotion habit: when an idea stabilizes, copy it into
> [`architecture.md`](architecture.md) §13 as a decision `D#` (or a real
> GitHub issue/milestone), mark the entry below `→ promoted: D#`, then delete
> it. Everything else stays comfortably throwaway.

## Personal Suggestions

> NOTE: very subjective and biased. Section numbers (§000–§005) are local to
> this file — unrelated to `architecture.md`'s sections.

### §000. Nothing is perfect

Grammars are mostly neutral, but can be well- or badly designed. A well-designed
grammar can let speakers express themselves naturally and describe things
elegantly, but it cannot stop one from spoiling it. A badly designed grammar
just makes spoiling easy.

### §001. Don't use `choice`/`case` if you have an alternative option

The `choice` and `case` statements are badly designed for NETCONF/YANG. If you
have ever worked on YANG compilation or NETCONF server/client development, you
know what I mean.

Let's suppose the industry really does want this feature: a single statement
could replace both of these, like so:

```yang
container alice {
    /*
        A container substatement whose default argument is `false`;
        when `true`, it makes the child branches single-selected.
    */
    single-selection true;

    leaf  option-1 {...}
    container option-2 {...}
    list option-3 {...}
}
```

With an XML encoding like:

```xml
<alice xmlns:maybe-yang-v2="who-knows-when" maybe-yang-v2:single-selection="true">
    <!-- do your choice -->
</alice>
```

And a JSON one like:

```json
{
    "alice" : {
        "some-invalid-symbol-for-yang-identifier-such-as@?:single-selection" : true
    }
}
```

If you reply that nested and sibling `choice`s are what you need, or that the
above would deepen the data tree and cost performance — blah blah — then
you're not wrong. But we both know the real problem is the modeling brain,
not the grammar.

*Open question (for me): is `single-selection` schema metadata (so it never
needs to appear on the wire) or runtime metadata that must be serialized? The
XML/JSON encodings above assume the latter — worth pinning down before
proposing.*

### §002. Don't use `submodule` if you have an alternative option

The problem here is that, semantically, `grouping`/`uses` is almost equivalent
to `submodule`, and `include`/`belongs-to` is somewhat duplicated.

If your reply is, *"hey, I do want to put a bunch of concepts into a single
namespace, but there are too many of them, so I have no choice but to split
them into a module/submodule tree"* — so that readers get a "top-level cleaner"
view from the entry module and need extra semantic tools to navigate between
files, getting lost in a typedef/identity/leafref forest?<br>

I suggest you rework your modeling design, sincerely.

### §003. Don't use `uses-augment` / `refine` if you have an alternative option

I won't bother explaining; they smell like patch semantics bolted onto
something that was misdesigned from the start. If that's really the case, the
fix is organizational/commercial, not grammatical.

### §004. Don't use the plus sign (`+`) for string concatenation

Just don't. If whitespace handling or word-wrap ever becomes the deciding
factor, the model is shit.

### §005. Limit XPath in `must`/`when` statements

Or better yet, don't use them at all.

## Future directions for this language server

### 0x001 A semantic IR / standard YANG compile pipeline

From my point of view, what the NETCONF ecosystem — or community, if there is
one — lacks is a semantic IR (intermediate representation) or a standard
compilation process for YANG. [yrepo](https://github.com/trislu/yrepo) is a
draft toward that ideal.

If such a standard ever comes out — or if I get spare time in the future — I
will consider refactoring this language server so it becomes
*yang-library-transparent*: users could pick their YANG-library build tools as
they wish, or not expose the actual YANG at all (vendors may introduce their own
magical extensions).

### 0x002 Introduce scripting for vendor extensions

A pyang-style plugin mechanism would genuinely help people who build on YANG
— statistics, analysis, extension handling. I'm not trying to redo what pyang
already does well; I just think scripting is a future requirement this project
can't avoid.

I looked into Lua-in-Rust and gave up. pyang plugins are written in Python, so
its extension story has no language friction. Bolting a scripting language
onto a Rust implementation would drag in too much: the meta-info, the semantic
phases, the Lua environment — more than I can picture ever stabilizing. So I'm
leaving it alone until some standard defines a protocol for this.

## Log

<!-- Date-stamped, low-ceremony entries. Prefix each with [id-NN]. -->

## 2026-09-05

- [id-01] YANG grammar critiques: single-selection instead of `choice`/`case`
  (§001), `submodule` duplication (§002), `uses-augment`/`refine` smell (§003).
  Subjective.
- [id-02] Direction (potential milestone): a semantic IR / standard YANG compile
  pipeline; possibly make the LS `yang-library-transparent` (yrepo as the
  draft). Not committed.

## 2026-09-08

- [id-03] Fleshed out §0x002 (vendor-extension scripting): a pyang-style plugin
  mechanism considered, then shelved — Lua-in-Rust ruled out (runtime/plugin
  friction, unstable surface); left until a standard defines the protocol.

## 2026-09-09

- [id-04] Concrete engineering plan for multi-workspace (multi-root) support:
  Option A (one merged logical tree) recommended; ~4–6 focused days in five
  phases. Tracked in [`docs/multi-root-workspaces.md`](multi-root-workspaces.md).

<!-- Promoted examples:
- [id-00] … → promoted: D31 (architecture §13/§14) — removed from this log.
-->
