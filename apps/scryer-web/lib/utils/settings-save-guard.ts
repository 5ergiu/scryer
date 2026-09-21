// A synchronous save lock also invalidates reads started before a mutation.
export class SettingsSaveGuard {
  private revision = 0;
  private saving = false;

  begin(): boolean {
    if (this.saving) return false;
    this.saving = true;
    this.revision += 1;
    return true;
  }

  end(): void {
    this.saving = false;
    this.revision += 1;
  }

  readVersion(): number | null {
    return this.saving ? null : this.revision;
  }

  accepts(version: number | null): boolean {
    return version !== null && !this.saving && version === this.revision;
  }
}
