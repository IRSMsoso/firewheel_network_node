# TODO

- [ ] Resample before passing to opus encoder / from opus decoder (To get rid of 48000 device sample rate requirement)
- [X] Consolidate 1 and 2 channel encoding logic
- [X] Test 2 channel encode/decode
- [ ] General cleanup
- [ ] Add examples for steam networking transport
- [ ] Recover more gracefully when internal buffer is depleted to avoid clicks when not getting enough network input in
  time