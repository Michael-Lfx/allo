import { LookField } from '../styleCatalog/LookPicker';

interface VisualStyleSelectProps {
  value: string;
  onChange: (stylePrompt: string) => void;
  disabled?: boolean;
}

export default function VisualStyleSelect({ value, onChange, disabled }: VisualStyleSelectProps) {
  return <LookField stylePrompt={value} onSelect={onChange} disabled={disabled} />;
}
