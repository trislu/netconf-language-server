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

> NOTE: very subjective and biased. Section numbers (§000–§003) are local to
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

Let's say if the telecommunications industry indeed want this feature: one statement
could beat these two, implemented like this:

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

If you reply that nested and sibling `choice`s are what you need, and that the
above would increase the data-tree depth and cause performance issues, blah
blah — then you're right! But we all know the problem is your modeling brain,
not the grammar.

*Open question (for me): is `single-selection` schema metadata (so it never
needs to appear on the wire) or runtime metadata that must be serialized? The
XML/JSON encodings above assume the latter — worth pinning down before
proposing.*

### §002. Don't use `submodule` if you have an alternative option

The problem here is that, semantically, `grouping`/`uses` is almost equivalent
to `submodule`, and `include`/`belongs-to` is somewhat duplicated.

If you reply, *"hey, I do want to put a bunch of concepts into a single
namespace, but there are too many of them, so I have no choice but to split
them into a module/submodule tree"* — so that readers get a "top-level cleaner"
view from the entry module and need extra semantic tools to navigate between
files, getting lost in a typedef/identity/leafref forest?<br>

I suggest you rework your modeling design, sincerely.

### §003. Don't use `uses-augment` / `refine` if you have an alternative option

I don't want to explain; they smell like a patch semantic for something not
correctly designed in the first place. If that is indeed the case, I believe the
solution is organizational/commercial, not grammatical.

## Future lean on this language server design

From my point of view, what the NETCONF ecosystem — or community, if there is
one — lacks is a semantic IR (intermediate representation) or a standard
compilation process for YANG. [yrepo](https://github.com/trislu/yrepo) is a
draft toward that ideal.

If such a standard ever comes out — or if I get spare time in the future — I
will consider refactoring this language server so it becomes
*yang-library-transparent*: users could pick their YANG-library build tools as
they wish, or not expose the actual YANG at all (vendors may introduce their own
magical extensions).

## Log

<!-- Date-stamped, low-ceremony entries. Prefix each with [id-NN]. -->

## 2026-09-05

- [id-01] YANG grammar critiques: single-selection instead of `choice`/`case`
  (§001), `submodule` duplication (§002), `uses-augment`/`refine` smell (§003).
  Subjective.
- [id-02] Direction (potential milestone): a semantic IR / standard YANG compile
  pipeline; possibly make the LS `yang-library-transparent` (yrepo as the
  draft). Not committed.

<!-- Promoted examples:
- [id-00] … → promoted: D31 (architecture §13/§14) — removed from this log.
-->
