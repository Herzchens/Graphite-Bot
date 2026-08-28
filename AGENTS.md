# AGENTS.md

Bộ quy tắc này áp dụng cho **mọi AI agent** (bao gồm ChatGPT 5.6 Sol High) khi được giao nhiệm vụ
sửa code, tạo branch, hoặc commit vào repo `Herzchens/Graphite-Bot`.

## 1. Nguyên tắc chung

- Giữ lịch sử commit **gọn gàng, dễ đọc, dễ revert**.
- Mỗi commit là một đơn vị thay đổi **hoàn chỉnh và có ý nghĩa** — không commit code dở dang, không commit "để lưu tạm".
- Không được tự ý push thẳng lên `main`/`master`. Luôn làm việc trên branch riêng rồi mở PR.

## 2. Cấm spam commit vụn vặt

Nghiêm cấm tạo chuỗi nhiều commit nhỏ kiểu:

```text
fix: typo
fix: typo again
test: debug
test: try again
wip
wip 2
```

Thay vào đó:

- Trong lúc phát triển/debug cục bộ, agent có thể commit tạm trên branch của mình, nhưng **trước khi mở PR hoặc merge phải squash lại** thành các commit có ý nghĩa (dùng `git rebase -i` hoặc `git reset --soft` + commit lại).
- Không tạo commit chỉ để "test xem có chạy không". Chạy test cục bộ trước, commit sau. Nếu lỡ push lên rồi mà fix nhỏ kiểu fix typo hay trong scope đó thì buộc amend và force push with lease.
- Nếu agent tự động hoá quy trình (CI, script), commit "auto-fix" phải được gộp lại thành 1 commit duy nhất trước khi đưa vào lịch sử chính, không giữ từng bước lặt vặt.

## 3. Quy ước đặt tên commit (Conventional Commits)

Format: `<type>(<scope tuỳ chọn>): <mô tả ngắn gọn, rõ ràng>`

**COMMIT BẰNG TIẾNG ANH.**

| Type | Ý nghĩa |
| --- | --- |
| `feat` | Thêm tính năng mới |
| `fix` | Sửa lỗi (chỉ 1 commit fix cho 1 lỗi cụ thể, không lặp) |
| `refactor` | Tái cấu trúc code, không đổi hành vi |
| `perf` | Cải thiện hiệu năng |
| `docs` | Thay đổi tài liệu |
| `chore` | Việc vặt (cập nhật dependency, config...) |
| `test` | Thêm/sửa test (chỉ khi test đã pass, không commit test đang fail) |
| `ci` | Thay đổi pipeline CI/CD |

Ví dụ tốt:

```text
feat(order-engine): add retry logic for failed orders
fix(auth): refresh expired tokens correctly
```

Ví dụ KHÔNG được chấp nhận:

```text
fix
update
asdf
test123
```

## 4. Cấm đặt tên branch theo tên agent/AI tool

**Nghiêm cấm tuyệt đối** các prefix branch sau (và mọi biến thể tương tự):

- `codex/...`
- `chatgpt/...`
- `agent/...`
- `ai/...`, `bot/...`, `gpt/...`, `claude/...`

Lý do: branch phải phản ánh **nội dung công việc**, không phải công cụ nào tạo ra nó.

Quy ước đặt tên branch bắt buộc:

```text
<type>/<mo-ta-ngan-gon-kebab-case>
```

Ví dụ:

```text
feat/add-retry-logic
fix/token-refresh-bug
refactor/order-engine-cleanup
```

## 5. Trước khi mở Pull Request

- [ ] Đã squash các commit vụn vặt thành commit(s) có ý nghĩa.
- [ ] Message commit tuân theo Conventional Commits.
- [ ] Tên branch không chứa prefix agent/AI bị cấm.
- [ ] Đã chạy test/lint cục bộ, không còn commit "debug"/"wip" trong lịch sử.
- [ ] Mô tả PR nêu rõ: làm gì, tại sao, ảnh hưởng gì.

## 6. Xử lý vi phạm

Nếu agent phát hiện mình đã tạo nhiều commit vụn vặt hoặc đặt sai tên branch:

1. Rebase/squash lại lịch sử commit trước khi push.
2. Nếu branch sai tên: tạo branch mới đúng chuẩn, di chuyển commit sang, xoá branch cũ.
3. Không được force-push đè lên lịch sử mà người khác đã dựa vào (branch chia sẻ) — chỉ force-push trên branch riêng của mình.
