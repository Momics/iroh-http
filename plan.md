
Loose notes: feel free to structure them:


- Streamline the folder structures and professionalize the setup. E.g. nodejs still have index.ts file, and no guest-js. Tauri app has guest-js and ts inside of it, while deno uses guest-ts etc. We need to think this through.
- For Tauri we need to design it as a plugin. See [this plugin](.old_references/iroh-tauri) as a reference.
- This will become an open source repository and each package needs to land in the respective place. That also means each needs a README.md file probably with clear instructions on how to use it and set things up. (tauri needs permissions and rust+js instructions, deno, nodejs, python all have different instructions probably). We want to publish on npm and jsr (and rust/python as well?) Please think this through carefully.
- Root README.md is also needed.
- Create a checklist before we opensource this thing. What is needed?
- Add example apps for each platform in [this directory](examples).