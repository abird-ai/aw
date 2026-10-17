export function Icon(props) {
  const size = props.size || 15;
  const common = {
    width: size,
    height: size,
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    "stroke-width": 1.7,
    "stroke-linecap": "round",
    "stroke-linejoin": "round",
    "aria-hidden": "true",
  };
  const paths = {
    search: <><circle cx="11" cy="11" r="7"/><path d="m20 20-3.2-3.2"/></>,
    file: <><path d="M6 2.8h7l5 5V21H6z"/><path d="M13 2.8V8h5"/></>,
    folder: <path d="M3 6.5h7l2 2h9v10.8a1.7 1.7 0 0 1-1.7 1.7H4.7A1.7 1.7 0 0 1 3 19.3z"/>,
    git: <><circle cx="6" cy="5" r="2"/><circle cx="6" cy="19" r="2"/><circle cx="18" cy="12" r="2"/><path d="M8 5h3a3 3 0 0 1 3 3v1a3 3 0 0 0 3 3M6 7v10"/></>,
    branch: <><circle cx="6" cy="5" r="2"/><circle cx="6" cy="19" r="2"/><circle cx="18" cy="7" r="2"/><path d="M6 7v10M8 5h3a4 4 0 0 1 4 4v2"/></>,
    tree: <><path d="M5 5h4M5 12h4M5 19h4M9 5v14M9 12h7M9 19h7"/><circle cx="18" cy="12" r="2"/><circle cx="18" cy="19" r="2"/></>,
    list: <><path d="M9 6h11M9 12h11M9 18h11"/><path d="M4 6h.01M4 12h.01M4 18h.01"/></>,
    plus: <path d="M12 5v14M5 12h14"/>,
    settings: <><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.6 1.6 0 0 0 .3 1.8l.1.1-2.8 2.8-.1-.1a1.6 1.6 0 0 0-1.8-.3 1.6 1.6 0 0 0-1 1.5V21h-4v-.2a1.6 1.6 0 0 0-1-1.5 1.6 1.6 0 0 0-1.8.3l-.1.1-2.8-2.8.1-.1a1.6 1.6 0 0 0 .3-1.8 1.6 1.6 0 0 0-1.5-1H3v-4h.2a1.6 1.6 0 0 0 1.5-1 1.6 1.6 0 0 0-.3-1.8l-.1-.1 2.8-2.8.1.1a1.6 1.6 0 0 0 1.8.3 1.6 1.6 0 0 0 1-1.5V3h4v.2a1.6 1.6 0 0 0 1 1.5 1.6 1.6 0 0 0 1.8-.3l.1-.1 2.8 2.8-.1.1a1.6 1.6 0 0 0-.3 1.8 1.6 1.6 0 0 0 1.5 1h.2v4h-.2a1.6 1.6 0 0 0-1.4 1z"/></>,
    close: <path d="m7 7 10 10M17 7 7 17"/>,
    chevronRight: <path d="m9 6 6 6-6 6"/>,
    chevronDown: <path d="m6 9 6 6 6-6"/>,
    command: <><rect x="3" y="5" width="18" height="14" rx="3"/><path d="m8 10 2 2-2 2M13 14h3"/></>,
    chat: <path d="M4 5h16v11H9l-5 4z"/>,
    refresh: <><path d="M20 6v5h-5"/><path d="M4 18v-5h5"/><path d="M6.1 8a7 7 0 0 1 11.5-1.9L20 11M4 13l2.4 4.9A7 7 0 0 0 17.9 16"/></>,
    popout: <><rect x="3.5" y="9" width="11.5" height="11.5" rx="1.5"/><path d="M12 4h8v8M20 4l-10 10"/></>,
  };
  return <svg {...common} class={props.class || ""}>{paths[props.name] || paths.file}</svg>;
}
