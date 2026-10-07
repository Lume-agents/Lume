import type { WorkflowGroupDefinition } from "./domain";

/** Serialize preference writes, coalescing rapid edits without dropping other groups. */
export class WorkflowBoardSaveQueue {
  private pending = new Map<string, WorkflowGroupDefinition>();
  private running: Promise<void> | null = null;

  private write: (group: WorkflowGroupDefinition) => Promise<void>;

  constructor(write: (group: WorkflowGroupDefinition) => Promise<void>) {
    this.write = write;
  }

  hasPending(id: string): boolean {
    return this.pending.has(id);
  }

  enqueue(group: WorkflowGroupDefinition): Promise<void> {
    this.pending.set(group.id, group);
    return this.flush();
  }

  flush(): Promise<void> {
    if (this.running) return this.running;
    if (!this.pending.size) return Promise.resolve();
    const operation = this.drain();
    this.running = operation.then(
      () => {
        this.running = null;
        // An edit can arrive after drain resolves but before this microtask runs.
        if (this.pending.size) return this.flush();
      },
      (error) => { this.running = null; throw error; },
    );
    return this.running;
  }

  private async drain(): Promise<void> {
    while (this.pending.size) {
      const [id, group] = this.pending.entries().next().value!;
      // Retain the newest edit on failure for an explicit retry.
      await this.write(group);
      if (this.pending.get(id) === group) this.pending.delete(id);
    }
  }
}
