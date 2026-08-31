interface WorkspaceEmptyProps {
  title: string;
  text: string;
  action?: string;
  onAction?: () => void;
}

export function WorkspaceEmpty({ title, text, action, onAction }: WorkspaceEmptyProps) {
  return (
    <div className="workspace-empty">
      <span>◇</span>
      <strong>{title}</strong>
      <p>{text}</p>
      {action && onAction && <button className="primary" onClick={onAction}>{action}</button>}
    </div>
  );
}
