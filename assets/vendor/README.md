# Local browser dependencies

Three.js **0.180.0**, MIT licensed, downloaded from the official npm registry tarball at `https://registry.npmjs.org/three/-/three-0.180.0.tgz`.

Only the ES-module renderer/core builds, GLTFLoader, its BufferGeometryUtils dependency, RoomEnvironment, and the original licence are included. Package metadata, licence, and addon imports were inspected before extraction. No package scripts were executed and no npm installation was performed. Files are unmodified upstream copies.

The page's import map resolves `three` and `three/addons/` locally. No framework, CDN, telemetry, decoder download, or runtime package resolution is used. Geometry compression is not required by the prepared model.

## Accessible dialogs

A11y Dialog **8.1.5**, MIT licensed, downloaded from `https://registry.npmjs.org/a11y-dialog/-/a11y-dialog-8.1.5.tgz`. The archive's SHA-512 was verified against npm metadata before extracting the unmodified, self-contained `dist/a11y-dialog.esm.js` and licence. The bundled focusable-selectors **0.8.0** licence is also included. Source and licences were reviewed; no package scripts or installation hooks were executed.

The ES module is served locally and handles modal focus trapping, Escape dismissal, and focus restoration. Its SHA-256 is `e384db98aac859596899f60c16c589604e8f92b2adea1652bb0645ad4339b83c`.

## Fonts

Fonts in `../fonts/` are locally hosted Red Hat Display and Red Hat Text, copied from the existing system font installation with their SIL Open Font Licences. The selected regular, medium, and semibold weights are unmodified.
