import type { ReactNode } from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useI18n } from "@/lib/i18n";

interface ConfirmDialogProps {
  open: boolean;
  title: string;
  description?: ReactNode;
  confirmLabel?: string;
  cancelLabel?: string;
  destructive?: boolean;
  loading?: boolean;
  onConfirm: () => void;
  onOpenChange: (open: boolean) => void;
}

// In-app replacement for the native confirm() dialogs.
export function ConfirmDialog({
  open,
  title,
  description,
  confirmLabel,
  cancelLabel,
  destructive = true,
  loading = false,
  onConfirm,
  onOpenChange,
}: ConfirmDialogProps) {
  const { t } = useI18n();
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-sm">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          {description ? <DialogDescription>{description}</DialogDescription> : null}
        </DialogHeader>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={loading}>
            {cancelLabel ?? t("common.cancel")}
          </Button>
          <Button
            variant={destructive ? "destructive" : "default"}
            onClick={onConfirm}
            disabled={loading}
          >
            {confirmLabel ?? t("common.confirm")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

interface PromptDialogProps {
  open: boolean;
  title: string;
  description?: ReactNode;
  fields: {
    key: string;
    label: string;
    type?: "text" | "password";
    placeholder?: string;
    defaultValue?: string;
    autoComplete?: string;
  }[];
  confirmLabel?: string;
  loading?: boolean;
  onSubmit: (values: Record<string, string>) => void;
  onOpenChange: (open: boolean) => void;
}

// In-app replacement for the native prompt() chains: a small form dialog
// returning every field's value on submit.
export function PromptDialog({
  open,
  title,
  description,
  fields,
  confirmLabel,
  loading = false,
  onSubmit,
  onOpenChange,
}: PromptDialogProps) {
  const { t } = useI18n();
  const values = fields.map((f) => f.defaultValue ?? "");
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-sm">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          {description ? <DialogDescription>{description}</DialogDescription> : null}
        </DialogHeader>
        <form
          className="flex flex-col gap-3"
          onSubmit={(e) => {
            e.preventDefault();
            const out: Record<string, string> = {};
            for (const [i, f] of fields.entries()) {
              const input = (e.currentTarget.elements.namedItem(f.key) as HTMLInputElement) ?? null;
              out[f.key] = input?.value ?? values[i];
            }
            onSubmit(out);
          }}
        >
          {fields.map((f, idx) => (
            <div key={f.key} className="flex flex-col gap-1.5">
              <Label htmlFor={`prompt-${f.key}`}>{f.label}</Label>
              <Input
                id={`prompt-${f.key}`}
                name={f.key}
                type={f.type ?? "text"}
                placeholder={f.placeholder}
                defaultValue={f.defaultValue}
                autoComplete={f.autoComplete}
                autoFocus={open && idx === 0}
                required
              />
            </div>
          ))}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)} disabled={loading}>
              {t("common.cancel")}
            </Button>
            <Button type="submit" disabled={loading}>
              {confirmLabel ?? t("common.confirm")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
