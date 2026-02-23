import { useState } from 'react';
import { Plus, Trash2, Eye, EyeOff } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';

interface EnvVarsFormProps {
  envVars: Record<string, string>;
  onChange: (envVars: Record<string, string>) => void;
  disabled?: boolean;
}

interface EnvRow {
  id: number;
  key: string;
  value: string;
}

let nextId = 0;

function toRows(envVars: Record<string, string>): EnvRow[] {
  const entries = Object.entries(envVars);
  if (entries.length === 0) return [];
  return entries.map(([key, value]) => ({ id: nextId++, key, value }));
}

function toRecord(rows: EnvRow[]): Record<string, string> {
  const record: Record<string, string> = {};
  for (const row of rows) {
    const key = row.key.trim();
    if (key) {
      record[key] = row.value;
    }
  }
  return record;
}

export function EnvVarsForm({ envVars, onChange, disabled }: EnvVarsFormProps) {
  const [rows, setRows] = useState<EnvRow[]>(() => toRows(envVars));
  const [visibleValues, setVisibleValues] = useState<Set<number>>(new Set());

  const updateRows = (newRows: EnvRow[]) => {
    setRows(newRows);
    onChange(toRecord(newRows));
  };

  const addRow = () => {
    updateRows([...rows, { id: nextId++, key: '', value: '' }]);
  };

  const removeRow = (id: number) => {
    const newRows = rows.filter((r) => r.id !== id);
    setVisibleValues((prev) => {
      const next = new Set(prev);
      next.delete(id);
      return next;
    });
    updateRows(newRows);
  };

  const updateKey = (id: number, key: string) => {
    const newRows = rows.map((r) =>
      r.id === id ? { ...r, key: key.toUpperCase().replace(/[^A-Z0-9_]/g, '') } : r,
    );
    updateRows(newRows);
  };

  const updateValue = (id: number, value: string) => {
    const newRows = rows.map((r) => (r.id === id ? { ...r, value } : r));
    updateRows(newRows);
  };

  const toggleVisibility = (id: number) => {
    setVisibleValues((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  };

  return (
    <div className="space-y-2">
      {rows.map((row) => (
        <div key={row.id} className="flex gap-2 items-center">
          <Input
            placeholder="VARIABLE_NAME"
            value={row.key}
            onChange={(e) => updateKey(row.id, e.target.value)}
            disabled={disabled}
            className="flex-1 font-mono text-xs h-8"
          />
          <div className="flex-1 relative">
            <Input
              placeholder="value"
              type={visibleValues.has(row.id) ? 'text' : 'password'}
              value={row.value}
              onChange={(e) => updateValue(row.id, e.target.value)}
              disabled={disabled}
              className="font-mono text-xs h-8 pr-8"
            />
            <button
              type="button"
              onClick={() => toggleVisibility(row.id)}
              className="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
              tabIndex={-1}
            >
              {visibleValues.has(row.id) ? (
                <EyeOff className="h-3.5 w-3.5" />
              ) : (
                <Eye className="h-3.5 w-3.5" />
              )}
            </button>
          </div>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => removeRow(row.id)}
            disabled={disabled}
            className="h-8 w-8 p-0 text-muted-foreground hover:text-destructive"
          >
            <Trash2 className="h-3.5 w-3.5" />
          </Button>
        </div>
      ))}
      <Button
        variant="outline"
        size="sm"
        onClick={addRow}
        disabled={disabled}
        className="h-7 text-xs"
      >
        <Plus className="h-3 w-3 mr-1" />
        添加变量
      </Button>
    </div>
  );
}
