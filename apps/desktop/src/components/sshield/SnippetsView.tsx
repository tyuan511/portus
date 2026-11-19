import { useState } from "react";
import { Check, Code2, Copy, Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import type { CommandSnippet } from "@/types";

interface SnippetsViewProps {
  snippets: CommandSnippet[];
  onSaveSnippet: (snippet: CommandSnippet) => Promise<void>;
  onDeleteSnippet: (id: string) => Promise<void>;
}

export function SnippetsView({ snippets, onSaveSnippet, onDeleteSnippet }: SnippetsViewProps) {
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [command, setCommand] = useState("");
  const [description, setDescription] = useState("");
  const [copiedId, setCopiedId] = useState<string | null>(null);

  const handleSave = async (e: React.FormEvent) => {
    e.preventDefault();
    await onSaveSnippet({
      id: "",
      name,
      command,
      description: description.trim() || null,
      tags: [],
    });
    setOpen(false);
    setName("");
    setCommand("");
    setDescription("");
  };

  return (
    <div className="flex-1 space-y-5 overflow-y-auto p-6">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-xl font-semibold text-foreground">Termius Snippets · 常用命令片段</h2>
          <p className="text-xs text-muted-foreground">沉淀高频只读排查命令，供人工与 AI 代理复用。</p>
        </div>
        <Button size="sm" className="bg-indigo-600 text-foreground hover:bg-indigo-500" onClick={() => setOpen(true)}>
          <Plus className="mr-1.5 size-4" /> New Snippet
        </Button>
      </div>

      {snippets.length === 0 ? (
        <Card className="border-border bg-card">
          <CardContent className="flex flex-col items-center py-14 text-center">
            <Code2 className="mb-3 size-9 text-muted-foreground" />
            <div className="text-sm text-foreground/90">暂无命令片段</div>
            <p className="mt-1 text-xs text-muted-foreground">点击右上角 New Snippet 创建常用运维诊断脚本。</p>
          </CardContent>
        </Card>
      ) : (
        <div className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
          {snippets.map((s) => (
            <Card key={s.id} className="border-border bg-card">
              <CardHeader className="pb-2">
                <div className="flex items-start justify-between">
                  <div>
                    <CardTitle className="text-sm text-foreground">{s.name}</CardTitle>
                    {s.description && <CardDescription className="text-xs">{s.description}</CardDescription>}
                  </div>
                  <div className="flex gap-1">
                    <Button
                      size="icon"
                      variant="ghost"
                      className="size-7 text-muted-foreground hover:text-foreground"
                      onClick={() => {
                        void navigator.clipboard.writeText(s.command);
                        setCopiedId(s.id);
                        setTimeout(() => setCopiedId(null), 1200);
                      }}
                    >
                      {copiedId === s.id ? <Check className="size-3.5 text-emerald-400" /> : <Copy className="size-3.5" />}
                    </Button>
                    <Button
                      size="icon"
                      variant="ghost"
                      className="size-7 text-muted-foreground hover:text-rose-300"
                      onClick={() => onDeleteSnippet(s.id)}
                    >
                      <Trash2 className="size-3.5" />
                    </Button>
                  </div>
                </div>
              </CardHeader>
              <CardContent>
                <pre className="overflow-x-auto rounded-lg bg-muted/60 p-2.5 font-mono text-xs text-emerald-300">
                  {s.command}
                </pre>
              </CardContent>
            </Card>
          ))}
        </div>
      )}

      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent className="border-border bg-card text-foreground sm:max-w-md">
          <DialogHeader>
            <DialogTitle>新建命令片段</DialogTitle>
            <DialogDescription>保存常用诊断命令到个人 Snippet 库。</DialogDescription>
          </DialogHeader>
          <form onSubmit={handleSave} className="space-y-3">
            <div className="space-y-1.5">
              <Label>名称</Label>
              <Input required placeholder="Docker 容器健康检查" value={name} onChange={(e) => setName(e.target.value)} />
            </div>
            <div className="space-y-1.5">
              <Label>命令内容</Label>
              <Textarea
                required
                rows={4}
                placeholder="docker ps --format 'table {{.Names}}\t{{.Status}}'"
                value={command}
                onChange={(e) => setCommand(e.target.value)}
                className="font-mono text-xs"
              />
            </div>
            <div className="space-y-1.5">
              <Label>说明 (可选)</Label>
              <Input value={description} onChange={(e) => setDescription(e.target.value)} />
            </div>
            <DialogFooter className="pt-2">
              <Button type="button" variant="outline" onClick={() => setOpen(false)}>
                取消
              </Button>
              <Button type="submit" className="bg-indigo-600 text-foreground hover:bg-indigo-500">
                保存片段
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </div>
  );
}
