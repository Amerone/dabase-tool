interface SectionHeaderProps {
  title: string;
  subtitle?: string;
}

export function SectionHeader({ title, subtitle }: SectionHeaderProps) {
  return (
    <div className="section-header">
      <h3 className="section-title">{title}</h3>
      {subtitle ? <p className="section-subtitle">{subtitle}</p> : null}
    </div>
  );
}
