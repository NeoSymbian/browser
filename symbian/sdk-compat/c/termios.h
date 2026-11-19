/* SPDX-License-Identifier: Apache-2.0 */
/* NeoSymbian: P.I.P.S. has no terminals. Enough termios for code that only asks (aws-lc's password prompt): the
 * calls fail with ENOTTY (browser std/cxxrt). */
#ifndef NEOSYMBIAN_TERMIOS_H
#define NEOSYMBIAN_TERMIOS_H
typedef unsigned int tcflag_t;
typedef unsigned char cc_t;
struct termios {
	tcflag_t c_iflag, c_oflag, c_cflag, c_lflag;
	cc_t c_cc[20];
};
#define ECHO 0x00000008
#define ICANON 0x00000100
#define ISIG 0x00000080
#define TCSANOW 0
#ifdef __cplusplus
extern "C" {
#endif
int tcgetattr(int fd, struct termios *t);
int tcsetattr(int fd, int action, const struct termios *t);
#ifdef __cplusplus
}
#endif
#endif
