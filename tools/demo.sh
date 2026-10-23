#!/usr/bin/env bash
# Construit un projet de démonstration via l'API du service.
#
# Sans argument, deux clips synthétiques sont générés avec ffmpeg : la
# démonstration fonctionne donc sur n'importe quelle machine, sans dépendre de
# fichiers personnels.
#
#   bash tools/demo.sh
#   VIDEO_A=/chemin/a.mp4 VIDEO_B=/chemin/b.mp4 bash tools/demo.sh
#   MKV=/chemin/film.mkv bash tools/demo.sh      # ajoute un média non lisible
#                                                # par le navigateur (proxy)
set -euo pipefail

API=${API:-http://127.0.0.1:8730}
DATA=${VELOCIS_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/velocis}
SAMPLES="$DATA/samples"
mkdir -p "$SAMPLES"

jq_get() { python3 -c "import sys,json;d=json.load(sys.stdin);print($1)"; }
post() { curl -sf -X POST "$API$1" -H 'content-type: application/json' -d "$2"; }

if ! curl -sf "$API/api/state" >/dev/null; then
  echo "Le service ne répond pas sur $API — lancez ./target/release/velocisd" >&2
  exit 1
fi

if [[ -z "${VIDEO_A:-}" ]]; then
  VIDEO_A="$SAMPLES/plan-large.mp4"
  if [[ ! -f "$VIDEO_A" ]]; then
    echo "→ génération d'un clip de démonstration"
    ffmpeg -hide_banner -loglevel error -y \
      -f lavfi -i "gradients=size=1280x720:rate=30:duration=8:speed=0.06" \
      -f lavfi -i "sine=frequency=220:sample_rate=48000:duration=8" \
      -c:v libx264 -preset veryfast -crf 26 -c:a aac -shortest "$VIDEO_A"
  fi
fi

if [[ -z "${VIDEO_B:-}" ]]; then
  VIDEO_B="$SAMPLES/incrustation.mp4"
  if [[ ! -f "$VIDEO_B" ]]; then
    ffmpeg -hide_banner -loglevel error -y \
      -f lavfi -i "testsrc2=size=960x540:rate=30:duration=6" \
      -f lavfi -i "sine=frequency=440:sample_rate=48000:duration=6" \
      -c:v libx264 -preset veryfast -crf 26 -c:a aac -shortest "$VIDEO_B"
  fi
fi

echo "→ nouveau projet"
post /api/project/new '{"name":"Démo Velocis","aspect":"landscape16x9","height":1080}' >/dev/null

echo "→ import"
A1=$(post /api/import "{\"paths\":[\"$VIDEO_A\"]}" | jq_get "d['imported'][0]['id']")
A2=$(post /api/import "{\"paths\":[\"$VIDEO_B\"]}" | jq_get "d['imported'][0]['id']")

T1=$(curl -sf "$API/api/state" | jq_get "d['project']['tracks'][0]['id']")

echo "→ clip principal (5 s, à partir de 1 s dans la source)"
post /api/clip/append "{\"track_id\":\"$T1\",\"asset_id\":\"$A1\",\"source_start\":1000000,\"source_duration\":5000000}" >/dev/null

echo "→ piste d'incrustation"
T2=$(post /api/track/add '{"kind":"video","name":"Incrustation"}' | jq_get "d['project']['tracks'][1]['id']")
C2=$(post /api/clip/place "{\"track_id\":\"$T2\",\"asset_id\":\"$A2\",\"timeline_start\":1200000,\"source_duration\":2500000}" | jq_get "d['project']['tracks'][1]['clips'][0]['id']")

echo "→ transformation et effets sur l'incrustation"
post /api/clip/update "{\"clip_id\":\"$C2\",\"opacity\":0.95,\"volume\":0.4,
  \"fade_in\":400000,\"fade_out\":400000,
  \"transform\":{\"x\":-380,\"y\":230,\"scale\":0.45,\"rotation_deg\":-3,
    \"mirror_x\":false,\"mirror_y\":false,
    \"crop_left\":0,\"crop_right\":0,\"crop_top\":0,\"crop_bottom\":0},
  \"effects\":[{\"kind\":\"saturation\",\"amount\":0.35},{\"kind\":\"vignette\",\"amount\":0.9}]}" >/dev/null

echo "→ piste de texte"
post /api/clip/text '{"content":"Démo Velocis","timeline_start":600000,"duration":3000000,"font_size":96}' >/dev/null

if [[ -n "${MKV:-}" ]]; then
  echo "→ média que le navigateur ne sait pas décoder (proxy à la demande)"
  AM=$(post /api/import "{\"paths\":[\"$MKV\"]}" | jq_get "d['imported'][0]['id']")
  curl -sf "$API/api/state" | python3 -c "
import sys, json
a = [x for x in json.load(sys.stdin)['project']['assets'] if x['id'] == '$AM'][0]
print('   ', a['name'], '|', round(a['duration']/1e6/60, 1), 'min |',
      a['video_codec'], '| lisible par le navigateur :', a['browser_playable'])
"
  post /api/clip/place "{\"track_id\":\"$T1\",\"asset_id\":\"$AM\",\"timeline_start\":7000000,\"source_start\":60000000,\"source_duration\":10000000}" >/dev/null
fi

echo "→ projet prêt : l'aperçu est composé en direct, rien à préparer"
curl -sf "$API/api/state" | python3 -c "
import sys, json
d = json.load(sys.stdin)['project']
print('projet :', d['name'], '|', d['canvas']['width'], 'x', d['canvas']['height'])
for t in d['tracks']:
    clips = [(round(c['timeline']['start']/1e6, 2), round(c['timeline']['duration']/1e6, 2),
              'texte' if c.get('text') else 'media', len(c['effects'])) for c in t['clips']]
    print(f'  {t[\"name\"]:<14} {clips}')
"
