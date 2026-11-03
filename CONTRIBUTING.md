# Contributing

Thank you for helping! MapSmithEE is an early preview and grows with what people ask for and share.

## Feedback, bugs and ideas

- **Bugs and suggestions:** open an [issue](https://github.com/sad0/MapSmithEE/issues/new/choose). For a bug, say what you
  did, what you expected and what happened; a screenshot helps a lot.
- **Ideas, questions, showing your maps:** [Discussions](https://github.com/sad0/MapSmithEE/discussions) or the
  [Discord](https://discord.gg/3XzeQv7HeS).

## Models and textures

Every model and texture must be free to share: **CC0** or **CC BY** (4.0 or 3.0), or your own work that you release under
one of them. No "editorial", "no derivatives" (ND) or paid-store assets.

### The easy way: an issue

Open a **New model or texture** issue with:

- the link (Sketchfab, Poly Haven, ambientCG, your own upload...),
- the author and the licence,
- what it is and where it belongs (category: nature, tents, furniture, supplies, fire and light, weapons, rugs,
  wagons and animals, buildings, remains),
- its real size if you know it (a barrel is about 0.9 m tall).

I add it to the model library, fit it to the game's scale and credit the author in [CREDITS.md](CREDITS.md).

### The hands-on way: a pull request

Add a folder under `contrib/models/<short_name>/` (or `contrib/textures/<short_name>/`) with:

- the model as one `.glb` (under 50 MB; textures inside, 2K at most), or the texture as `diff_2k.jpg` (+ `bump.jpg` if
  you have one; seamless),
- an `info.json`:

```json
{
  "name": "Old wooden cart",
  "author": "Author Name",
  "source": "https://sketchfab.com/3d-models/...",
  "licence": "CC BY 4.0",
  "category": "wagons",
  "size_m": [2.6, 1.2, 1.4]
}
```

`size_m` is width, height, depth in metres. The models are folded into the next model-library release (the library is
too big for git), with the author credited.

## Code

Pull requests are welcome. Keep changes focused, and say how you tested them. By contributing you agree that your work is
released under the project's licence ([CC BY-NC-SA 4.0](LICENSE)).
