# 0073 Task Completion Control Function 手動確認

実Gemini profileとAPI keyをユーザーが既に安全なaibe設定へ投入済みの場合だけ行う任意確認。keyやprovider bodyをrepo、fixture、command logへ保存しない。

1. 一時workspaceで`ai ask --task-completion`へ小さなfile作成とread-back検証を依頼する。
2. secretを表示しない診断で、Active turnの初回assistant stepがtextなし`set_task_contract` callで、definitionが一件だけであることを確認する。
3. control成功後にwrite、post-observation read、Contract fieldなしevaluationへ進み、Doneになることを確認する。
4. opt-inなしturnではcontrol definitionが公開されず、通常応答を維持することを確認する。
5. stdout/stderr/log/CompletionReport/保存conversationにraw control argsや秘密値がないことを確認する。

自動試験の代替にはしない。未実施時は完了報告の残リスクへ記載する。
