; Creates a circle on the XY plane at machine center

G54;
G90;
X0. Y0.; Move to center

G91;
G1 X100. F50.;

G90; Absolute mode
G2 X-100. Y0. Z200. I-100. J0.; Clockwise arc with center at G54 offset

G2 X0. Y100. Z150. I100. J0.; Clockwise arc with center at G54 offset

G2 X100. Y0. Z100. I0. J-100.; Clockwise arc with center at G54 offset

;G53 X0. Y0.;
M30;
