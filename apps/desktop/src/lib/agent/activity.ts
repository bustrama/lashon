export interface AgentActivity {
    id: number;
    title?: string | null;
    project?: string | null;
    agent: string;
    state: 'working' | 'tool' | 'done' | 'stopped' | 'error';
    tool: string | null;
}
