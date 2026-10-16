import test from 'node:test';
import assert from 'node:assert/strict';
import { renderMarkdown } from '../web/src/markdown.js';

test('rich markdown renders tables strike and task markers', () => {
  const html = renderMarkdown('| Name | State |\n| --- | --- |\n| bird | ready |\n\n- [ ] todo\n- [x] done\n\n~~old~~ new');
  assert.match(html, /<table>/);
  assert.match(html, /<s>old<\/s>/);
  assert.match(html, /class="task-item"[^>]*>☐ todo/);
  assert.match(html, /class="task-item done"[^>]*>☑ done/);
});

test('image markdown is an inert media reference and never an img fetch', () => {
  const html = renderMarkdown('![diagram](https://example.test/a.png)');
  assert.doesNotMatch(html, /<img\b/i);
  assert.match(html, /class="media-ref"/);
  assert.match(html, /\[image: diagram\]/);
  assert.match(html, /href="https:\/\/example\.test\/a\.png"/);
  assert.match(html, /noopener noreferrer/);
});

test('unsafe media destinations remain visible text without href', () => {
  const html = renderMarkdown('![x](data:text/html;base64,AAAA)');
  assert.doesNotMatch(html, /<img\b/i);
  assert.doesNotMatch(html, /href=/i);
  assert.match(html, /!\[x\]\(data:text\/html;base64,AAAA\)/);
});
