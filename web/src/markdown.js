import MarkdownIt from "markdown-it";

const md = new MarkdownIt({
  html: false,
  linkify: true,
  breaks: false,
  typographer: false,
});

const defaultValidate = md.validateLink.bind(md);
md.validateLink = url => defaultValidate(url) && !/^data:/i.test(url);

md.renderer.rules.link_open = (tokens, idx, options, env, self) => {
  const token = tokens[idx];
  token.attrSet("target", "_blank");
  token.attrSet("rel", "noopener noreferrer");
  return self.renderToken(tokens, idx, options);
};

md.renderer.rules.image = (tokens, idx) => {
  const token = tokens[idx];
  const source = token.attrGet("src") || "";
  const alt = token.content || "media";
  const safeAlt = md.utils.escapeHtml(alt || "media");
  const safeSource = md.utils.escapeHtml(source);
  if (!source || !md.validateLink(source)) {
    return `<span class="media-ref">[image: ${safeAlt}] (${safeSource})</span>`;
  }
  const href = md.utils.escapeHtml(md.normalizeLink(source));
  return `<span class="media-ref">[image: ${safeAlt}] (<a href="${href}" target="_blank" rel="noopener noreferrer">${safeSource}</a>)</span>`;
};

export function renderMarkdown(text = "") {
  return md
    .render(text)
    .replace(/<li>\[ \] /g, '<li class="task-item">☐ ')
    .replace(/<li>\[[xX]\] /g, '<li class="task-item done">☑ ');
}
