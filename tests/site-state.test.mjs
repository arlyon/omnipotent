import test from 'node:test';
import assert from 'node:assert/strict';
import { clamp, smoothstep, storyProgress, modelPose, screenStates } from '../js/state.mjs';
import { collections, collectionProgress, collectionSequence, collectionContent } from '../js/collections.mjs';
import { readFileSync } from 'node:fs';
import { catalogueMatches } from '../js/catalogue.mjs';

const catalogue = JSON.parse(readFileSync(new URL('../assets/data/xberg-formats.json', import.meta.url), 'utf8'));

test('catalogue search covers every format, aliases and MIME-only entries', () => {
  assert.ok(catalogue.formats.every(item => catalogueMatches(item, '   ')));
  assert.deepEqual(catalogue.formats.filter(item => catalogueMatches(item, '.DOCX')).map(item => item.mime),
    ['application/vnd.openxmlformats-officedocument.wordprocessingml.document']);
  assert.equal(catalogue.formats.find(item => catalogueMatches(item, 'application/x-jsonlines')).mime, 'application/x-ndjson');
  assert.equal(catalogue.formats.find(item => catalogueMatches(item, 'text/x-pandoc')).mime, 'text/x-pandoc');
  assert.equal(catalogue.formats.find(item => catalogueMatches(item, '.gpkx')).name, 'GeoPackage');
});

test('catalogue search supports multiple words and all code/configuration grammars', () => {
  assert.ok(catalogue.languages.every(item => catalogueMatches(item, '')));
  assert.equal(catalogue.languages.find(item => catalogueMatches(item, 'Rust')).id, 'rust');
  assert.equal(catalogue.languages.find(item => catalogueMatches(item, 'C++')).id, 'cpp');
  const templates = catalogue.formats.filter(item => catalogueMatches(item, 'word template'));
  assert.equal(templates.length, 2);
  assert.ok(templates.every(item => item.name.includes('template')));
});

test('catalogue search treats markup and regex characters as plain text', () => {
  const entries = [...catalogue.formats, ...catalogue.languages];
  for (const input of ['<img src=x onerror=alert(1)>', '.*', '[a-z]+']) {
    assert.equal(entries.filter(item => catalogueMatches(item, input)).length, 0);
  }
});

test('story and collection progress stay bounded', () => {
  assert.equal(storyProgress(100, 3000, 900), 0);
  assert.equal(storyProgress(-1050, 3000, 900), 0.5);
  assert.equal(storyProgress(-4000, 3000, 900), 1);
  assert.equal(storyProgress(0, 600, 900), 0);
  assert.equal(clamp(-5), 0); assert.equal(clamp(5), 1);
  assert.equal(smoothstep(0.5), 0.5);
  assert.equal(collectionProgress(450, 3000, 1000), 0);
  assert.equal(collectionProgress(-2000, 3000, 1000), 1);
});

test('open, seat, refresh, ready sequence remains face-on', () => {
  assert.equal(modelPose(0).seated, 0);
  assert.equal(modelPose(0).screen, 'blank');
  assert.ok(modelPose(0.17).seated > 0 && modelPose(0.17).seated < 1);
  assert.equal(modelPose(0.31).seated, 1);
  assert.equal(modelPose(0.31).screen, 'refresh');
  assert.equal(modelPose(0.4).screen, 'ready');
  let previous = 0;
  for (let i = 0; i <= 100; i += 1) {
    const pose = modelPose(i / 100);
    assert.ok(pose.seated >= previous);
    assert.equal(pose.yaw, 0); assert.equal(pose.roll, 0);
    if (pose.screen !== 'blank') assert.equal(pose.seated, 1);
    previous = pose.seated;
  }
});

test('manual overrides and reversed scroll return to blank', () => {
  assert.equal(modelPose(1, true).seated, 0);
  assert.equal(modelPose(1, true).screen, 'blank');
  assert.equal(modelPose(0, false).seated, 1);
  assert.equal(modelPose(0, false).screen, 'ready');
  assert.equal(modelPose(1).screen, 'ready');
  assert.equal(modelPose(0).screen, 'blank');
});

test('reduced motion is stable and completed', () => {
  assert.deepEqual(modelPose(0, null, true), modelPose(1, null, true));
  assert.equal(modelPose(1, null, true).seated, 1);
  assert.equal(modelPose(1, null, true).screen, 'ready');
  assert.equal(modelPose(1, true, true).seated, 0);
  assert.ok(collectionSequence(0.5, { reduced: true }).bubbles.every((bubble) => bubble.opacity === 0));
});

test('each count increments only when its bubble lands', () => {
  for (let item = 0; item < 6; item += 1) {
    const before = collectionSequence((item + 0.999) / 6);
    const after = collectionSequence((item + 1) / 6);
    assert.equal(before.loaded, item);
    assert.equal(after.loaded, item + 1);
    assert.equal(after.bubbles[item].landed, true);
    assert.equal(after.bubbles[item].opacity, 0);
    assert.equal(after.last.id, collections[item].id);
  }
  const complete = collectionSequence(1);
  assert.equal(complete.complete, true);
  assert.ok(complete.bubbles.every((bubble) => bubble.landed));
  assert.equal(collectionSequence(0).loaded, 0);
  assert.equal(collectionSequence(0.2).loaded, 1);
  assert.equal(collectionContent(complete).title, '06 / 06');
});

test('incoming bubble converges on the measured screen anchor', () => {
  const anchor = { x: 0.42, y: 0.37 };
  const bubble = collectionSequence(0.95 / 6, { anchor }).bubbles[0];
  assert.equal(bubble.x, anchor.x); assert.equal(bubble.y, anchor.y);
  assert.ok(bubble.scale < 0.3);
  for (let i = 0; i <= 100; i += 1) {
    const scene = collectionSequence(i / 100);
    for (const item of scene.bubbles) {
      assert.ok([item.x, item.y, item.scale, item.opacity].every(Number.isFinite));
      assert.ok(item.opacity >= 0 && item.opacity <= 1);
    }
  }
});

test('the six collections and reusable screen states are complete', () => {
  assert.equal(collections.length, 6);
  assert.equal(new Set(collections.map((item) => item.id)).size, 6);
  for (const item of collections) {
    for (const key of ['label', 'screen', 'format', 'description']) assert.ok(item[key].length > 0);
  }
  assert.deepEqual(Object.keys(screenStates), ['blank', 'refresh', 'ready', 'sleep', 'access', 'index']);
});
