# Harmony Cut

Local agent-led commercial editing workstation.

Harmony Cut is an MVP for:

- scanning a material folder;
- creating thumbnails and proxy clips;
- recommending stable in/out windows;
- writing review marker files into the material folder;
- reviewing clips in a local web UI with large, fast controls;
- saving an editable EDL after human review;
- rendering a quick local MP4 preview;
- generating a JianYing draft through the local `jianying-editor` wrapper.

## Run

```powershell
cd D:\harmony_cut
python server.py
```

Then open:

```text
http://127.0.0.1:8765
```

## Current MVP

- Native HTML/JS frontend, no Node build step.
- Starlette + Uvicorn backend.
- OpenCV-based media scan, thumbnail extraction, and sliding-window in/out scoring.
- Rotation proxy generation for camera-orientation mistakes.
- JSON project storage under `D:\harmony_cut\data\projects`.
- Material-folder marker files: `harmony_cut_edl.json` and `harmony_cut_edl.xml`.
- Agent prepare/continue APIs and CLI helpers.
- Folder browser endpoint for local drives and material folders.
- Manual asset-to-shot-slot assignment for the 12-shot timeline.
- Caption text splitting and basic clip-time matching.
- ffmpeg preview render under `D:\harmony_cut\data\previews`.
- JianYing draft creation endpoint.
- Large-button review UI for in/out, speed, shot role, rotation, and simple zoom keyframes.

## API

- `GET /` - review UI.
- `GET /?project=<project_id>` - review UI opened on a specific Agent-prepared project.
- `POST /api/agent/prepare` - Agent creates project, scans material, builds rough cut, writes marker files, and returns review URL.
- `POST /api/agent/continue` - Agent continues after user review, renders preview, creates JianYing draft, and updates marker files.
- `GET /api/browse?path=<folder>` - list drives or subfolders.
- `GET /api/projects` - list local projects.
- `POST /api/projects` - create a project.
- `GET /api/projects/{project_id}` - load project JSON.
- `POST /api/projects/{project_id}/scan` - scan materials and create an initial EDL.
- `POST /api/projects/{project_id}/save` - save edited EDL.
- `POST /api/projects/{project_id}/captions` - split script text and match captions to clips.
- `POST /api/projects/{project_id}/preview` - render a 540x960 preview MP4 from the current EDL.
- `POST /api/projects/{project_id}/draft` - create a JianYing draft.
- `GET /api/file?path=<absolute-path>` - serve local media or thumbnails.

## Project Flow

1. User gives Agent a material folder and editing requirement.
2. Agent calls:

```powershell
python D:\harmony_cut\agent_prepare.py --media-dir "D:\media\watch" --requirement "12-shot watch commercial"
```

3. Harmony Cut scans media, creates the rough cut, writes `harmony_cut_edl.json/xml` into the material folder, and returns a URL like:

```text
http://127.0.0.1:8765/?project=<project_id>
```

4. User opens the workbench and reviews each asset or timeline shot:
   - mark in/out;
   - put the selected asset into the selected timeline slot;
   - tag shot type;
   - set speed;
   - mark rotation;
   - set simple zoom keyframes.
5. User clicks `保存并回到 AI`; Harmony Cut updates the marker files and tells the user to send `继续`.
6. User returns to Agent and sends `继续`.
7. Agent calls:

```powershell
python D:\harmony_cut\agent_continue.py --project-id <project_id>
```

8. Harmony Cut renders a preview, creates a JianYing draft, and updates marker files.

## Next Development Targets

- Add persistent keyboard shortcut help and batch actions.
- Improve role classification so the 12-shot watch template uses fewer repeated angles.
- Add BGM discovery, waveform beat markers, and beat-aware duration snapping.
- Add direct `capcut-mate` draft generation as an alternative to JyWrapper.
- Add optional vision-model analysis for semantic labels such as opening, hero rotation, handheld, and wrist close-up.
