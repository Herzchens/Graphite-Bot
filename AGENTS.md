# AGENTS.md

Bộ quy tắc này áp dụng cho **mọi AI agent** (bao gồm ChatGPT 5.6 Sol High) khi được giao nhiệm vụ
sửa code, tạo branch, hoặc commit vào repo `Herzchens/Graphite-Bot`.

## 1. Nguyên tắc chung

- Giữ lịch sử commit **gọn gàng, dễ đọc, dễ revert**.
- Mỗi commit là một đơn vị thay đổi **hoàn chỉnh và có ý nghĩa** — không commit code dở dang, không commit "để lưu tạm".
- Không được tự ý push thẳng lên `main`/`master`. Luôn làm việc trên branch riêng rồi mở PR.
- Nếu môi trường agent không có checkout/toolchain local hoặc không cho phép cài thêm dependency, agent **không được giả vờ** đã chạy local verification. Hãy dùng GitHub API/connector để sửa source và dùng GitHub Actions read-only để verify final head.
- CI dùng để **kiểm chứng**, không dùng để tự sửa rồi commit/push code ngược lại branch PR. Không cấp `contents: write` chỉ để auto-format/auto-fix source.

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
- Không tạo commit chỉ để "test xem có chạy không". Nếu môi trường có local toolchain, chạy test trước rồi commit. Nếu agent chỉ có môi trường web/connector, dùng CI read-only trên branch và squash/amend mọi sửa nhỏ trước final verification.
- Nếu agent tự động hoá quy trình (CI, script), commit "auto-fix" phải được gộp lại thành 1 commit duy nhất trước khi đưa vào lịch sử chính, không giữ từng bước lặt vặt. Tốt hơn là tránh để CI tự ghi source ngay từ đầu.

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
- [ ] Đã chạy test/lint trong môi trường thực thi sẵn có. Nếu agent không có local checkout/toolchain, phải nói rõ và dùng CI read-only thay thế; không được claim local verification giả.
- [ ] Không còn commit `debug`/`wip`/auto-fix rác trong lịch sử.
- [ ] Mô tả PR nêu rõ: làm gì, tại sao, ảnh hưởng gì.

## 6. Bắt buộc self-review sau khi implementation hoàn tất

**CI xanh chưa đủ để coi công việc là hoàn thành.** Sau khi viết xong code và trước khi đánh dấu PR ready/merge, agent bắt buộc phải đọc lại chính phần mình vừa thay đổi như một reviewer độc lập.

Self-review tối thiểu phải kiểm tra:

- [ ] Đọc lại toàn bộ diff cuối cùng, không chỉ các file vừa sửa gần nhất.
- [ ] Đối chiếu implementation với specification/invariants nguồn; không tự bịa hành vi để lấp khoảng trống của spec.
- [ ] Kiểm tra transaction boundary, lock order, idempotency/retry, rollback và partial-failure để tránh half-commit hoặc double-apply.
- [ ] Kiểm tra integer overflow/underflow, rounding, boundary values, empty/zero/max cases và dữ liệu persistence không hợp lệ.
- [ ] Kiểm tra concurrency/race/deadlock khi code đụng state dùng chung hoặc PostgreSQL row locks.
- [ ] Kiểm tra security/trust boundary: input bên ngoài không được bypass invariant, lộ secret hoặc làm dữ liệu authoritative phụ thuộc state in-memory.
- [ ] Tìm code duplicate, abstraction tạm bợ, coupling sai layer, TODO/FIXME hoặc workaround có khả năng biến thành tech debt. Nếu nằm trong scope hiện tại và có thể sửa hợp lý thì sửa **trước khi merge**, không đẩy nợ sang phase sau chỉ để PR xanh nhanh.
- [ ] Kiểm tra test có chứng minh success path, rejection path và regression cho bug/edge case quan trọng; không dùng test chỉ để làm CI xanh.
- [ ] Kiểm tra docs/status không claim nhiều hơn code thật sự implement.
- [ ] Xác nhận branch cuối sạch, lịch sử commit có nghĩa và CI chạy trên **chính final head** sau mọi sửa đổi từ self-review.

Nếu self-review phát hiện vấn đề:

1. Sửa root cause trên branch hiện tại.
2. Bổ sung hoặc chỉnh regression test tương ứng.
3. Squash/amend lại để lịch sử cuối không chứa commit sửa-vặt do self-review.
4. Chạy lại toàn bộ gate bắt buộc trên final head.
5. Chỉ merge khi **CI xanh + self-review không còn blocker**.

Triết lý mặc định: **chậm mà chắc; ưu tiên correctness, maintainability và bằng chứng hơn tốc độ hoàn thành bề ngoài.**

## 7. Xử lý vi phạm

Nếu agent phát hiện mình đã tạo nhiều commit vụn vặt hoặc đặt sai tên branch:

1. Rebase/squash lại lịch sử commit trước khi push.
2. Nếu branch sai tên: tạo branch mới đúng chuẩn, di chuyển commit sang, xoá branch cũ.
3. Không được force-push đè lên lịch sử mà người khác đã dựa vào (branch chia sẻ) — chỉ force-push trên branch riêng của mình.
