import { useCallback, useEffect, useState } from "react";
import { Check, Copy, Pencil, Plus, Search, Terminal, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { ConfirmDialog } from "@/components/sshield/ConfirmDialog";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { ErrorText } from "@/components/ui/shared";
import { api } from "@/lib/api";
import { useAuth } from "@/lib/auth";
import { useI18n } from "@/lib/i18n";
import type { CommandSnippet } from "@/types";

interface SnippetForm {
  id: string;
  name: string;
  command: string;
  description: string;
  tags: string;
}

const emptyForm = (): SnippetForm => ({ id: "", name: "", command: "", description: "", tags: "" });

export default function Snippets() {
  const auth = useAuth();
  const { t } = useI18n();
  const canManage = auth.hasPermission("snippets.manage");
  const [snippets, setSnippets] = useState<CommandSnippet[]>([]);
  const [error, setError] = useState<string>();
  const [form, setForm] = useState<SnippetForm | null>(null);
  const [deleteSnippet, setDeleteSnippet] = useState<CommandSnippet | null>(null);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [query, setQuery] = useState("");

  const load = useCallback(async () => {
    try {
      setSnippets(await api.get<CommandSnippet[]>("/api/snippets"));
    } catch (err) {
      setError(err instanceof Error ? err.message : "load failed");
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const save = async () => {
    if (!form?.name || !form?.command) return;
    try {
      await api.post("/api/snippets", {
        id: form.id,
        name: form.name,
        command: form.command,
        description: form.description,
        tags: form.tags
          .split(",")
          .map((s) => s.trim())
          .filter(Boolean),
      });
      setForm(null);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "save failed");
    }
  };

  const remove = async (id: string) => {
    try {
      await api.del(`/api/snippets/${id}`);
      setDeleteSnippet(null);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "delete failed");
    }
  };

  const copy = async (id: string, command: string) => {
    await navigator.clipboard.writeText(command);
    setCopiedId(id);
    window.setTimeout(() => setCopiedId((cur) => (cur === id ? null : cur)), 1600);
  };

  const filtered = snippets.filter((s) => {
    const q = query.trim().toLowerCase();
    if (!q) return true;
    return (
      s.name.toLowerCase().includes(q) ||
      s.command.toLowerCase().includes(q) ||
      s.tags.some((tag) => tag.toLowerCase().includes(q))
    );
  });

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="relative w-full max-w-xs">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={t("snippets.search")}
            className="pl-8"
          />
        </div>
        {canManage && (
          <Button size="sm" onClick={() => setForm(emptyForm())}>
            <Plus data-icon="inline-start" />
            {t("snippets.new")}
          </Button>
        )}
      </div>
      <ErrorText>{error}</ErrorText>

      {filtered.length === 0 ? (
        <Empty className="border border-border bg-card py-12">
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <Terminal />
            </EmptyMedia>
            <EmptyTitle>{t("security.snippets")}</EmptyTitle>
            <EmptyDescription>{t("snippets.empty")}</EmptyDescription>
          </EmptyHeader>
          {canManage && (
            <EmptyContent>
              <Button size="sm" onClick={() => setForm(emptyForm())}>
                <Plus data-icon="inline-start" />
                {t("snippets.new")}
              </Button>
            </EmptyContent>
          )}
        </Empty>
      ) : (
        <div className="flex flex-col gap-2">
          {filtered.map((s) => (
            <Card key={s.id} size="sm" className="flex-row items-center justify-between gap-4 px-4 py-3">
              <div className="min-w-0">
                <div className="text-sm font-medium">{s.name}</div>
                <code className="mt-0.5 block truncate font-mono text-xs text-muted-foreground" title={s.command}>
                  {s.command}
                </code>
              </div>
              <div className="flex shrink-0 items-center gap-1.5">
                <Button variant="outline" size="xs" onClick={() => void copy(s.id, s.command)}>
                  {copiedId === s.id ? <Check data-icon="inline-start" /> : <Copy data-icon="inline-start" />}
                  {copiedId === s.id ? t("common.copied") : t("common.copy")}
                </Button>
                {canManage && (
                  <>
                    <Button variant="ghost" size="icon-xs" onClick={() => setForm({ id: s.id, name: s.name, command: s.command, description: s.description ?? "", tags: s.tags.join(", ") })}>
                      <Pencil />
                    </Button>
                    <Button variant="destructive" size="icon-xs" onClick={() => setDeleteSnippet(s)}>
                      <Trash2 />
                    </Button>
                  </>
                )}
              </div>
            </Card>
          ))}
        </div>
      )}

      <Dialog open={form !== null} onOpenChange={(o) => !o && setForm(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>
              {form?.id ? t("snippets.editTitle", { name: form.name }) : t("snippets.new")}
            </DialogTitle>
          </DialogHeader>
          <div className="flex flex-col gap-3">
            <div className="flex flex-col gap-1.5">
              <Label>{t("common.name")}</Label>
              <Input
                value={form?.name ?? ""}
                onChange={(e) => setForm({ ...form!, name: e.target.value })}
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label>{t("snippets.command")}</Label>
              <Input
                className="font-mono text-xs"
                value={form?.command ?? ""}
                onChange={(e) => setForm({ ...form!, command: e.target.value })}
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label>{t("common.description")}</Label>
              <Textarea
                className="min-h-16"
                value={form?.description ?? ""}
                onChange={(e) => setForm({ ...form!, description: e.target.value })}
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label>{t("snippets.tags")}</Label>
              <Input
                value={form?.tags ?? ""}
                onChange={(e) => setForm({ ...form!, tags: e.target.value })}
              />
            </div>
            <ErrorText>{error}</ErrorText>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setForm(null)}>
              {t("common.cancel")}
            </Button>
            <Button onClick={() => void save()} disabled={!form?.name || !form?.command}>
              {t("common.save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        open={deleteSnippet !== null}
        title={t("common.delete")}
        description={t("snippets.deleteConfirm")}
        confirmLabel={t("common.delete")}
        onConfirm={() => deleteSnippet && void remove(deleteSnippet.id)}
        onOpenChange={(o) => !o && setDeleteSnippet(null)}
      />
    </div>
  );
}
