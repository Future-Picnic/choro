"""Build editable Choro brand layouts and export them with ImageMagick.

Uses the existing production app icon unchanged, embedded in native SVG layouts.
Run from any directory. Does not delete files or touch application code.
"""
from pathlib import Path
from html import escape
import base64
import subprocess

ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / 'assets'
ICON = base64.b64encode((ASSETS / 'choro-profile-1024.png').read_bytes()).decode()

def text(x, y, value, size, color='#EDE9EA', weight='400', spacing='0'):
    return f'<text x="{x}" y="{y}" fill="{color}" font-family="Arial" font-size="{size}" font-weight="{weight}" letter-spacing="{spacing}">{escape(value)}</text>'

def background(w, h):
    return f'''<rect width="{w}" height="{h}" fill="#19181C"/>
    <path d="M {w*.66} {-h*.7} C {w*.28} {h*.15}, {w*1.08} {h*.6}, {w*.68} {h*1.7}" stroke="#36303F" stroke-width="{h*.14}" fill="none"/>
    <path d="M {w*.70} {-h*.7} C {w*.32} {h*.15}, {w*1.12} {h*.6}, {w*.72} {h*1.7}" stroke="#CAC9EE" stroke-opacity=".16" stroke-width="2" fill="none"/>
    <path d="M {w*.76} {-h*.7} C {w*.38} {h*.15}, {w*1.18} {h*.6}, {w*.78} {h*1.7}" stroke="#CAC9EE" stroke-opacity=".10" stroke-width="2" fill="none"/>'''

def icon(x,y,s):
    return f'<image x="{x}" y="{y}" width="{s}" height="{s}" xlink:href="data:image/png;base64,{ICON}"/>'

def write(name,w,h,body):
    svg=f'<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{w}" height="{h}" viewBox="0 0 {w} {h}">{body}</svg>'
    p=ASSETS / (name+'.svg')
    p.write_text(svg)
    subprocess.run(['magick','-background','#19181C',str(p),'-strip',str(ASSETS/(name+'.png'))],check=True)

# Critical YouTube content fits inside the central 1546 x 423 area.
write('youtube-banner-2560x1440',2560,1440,background(2560,1440)+
      icon(566,598,236)+text(856,654,'Choro',72,weight='700')+
      text(856,735,'One place to build your products.',45)+
      text(859,794,'WORKFLOWS  /  WALKTHROUGHS  /  WHAT’S NEW',21,'#CAC9EE',spacing='2'))

# Keep Page-avatar overlap clear at the left of the LinkedIn cover.
write('linkedin-cover-4200x700',4200,700,background(4200,700)+
      text(1120,240,'Choro',112,weight='700')+
      text(1120,375,'One place to build your products.',94)+
      text(1125,470,'Projects. Agents. Context. Together on your Mac.',42,'#CAC9EE')+
      text(3550,595,'choro.dev',34,'#CAC9EE'))

# Center the essential Facebook copy to tolerate desktop/mobile crops.
write('facebook-cover-1640x924',1640,924,background(1640,924)+
      text(430,320,'Choro',86,weight='700')+
      text(430,416,'One place to build',58)+text(430,492,'your products.',58)+
      text(433,564,'Your AI workspace on Mac.',28,'#CAC9EE'))

write('reddit-banner-4000x192',4000,192,background(4000,192)+
      text(1450,88,'Choro',59,weight='700')+
      text(1452,137,'BUILD  /  SHARE  /  LEARN',23,'#CAC9EE',spacing='3'))
write('reddit-mobile-1600x480',1600,480,background(1600,480)+
      text(460,225,'Choro',92,weight='700')+
      text(464,298,'BUILD  /  SHARE  /  LEARN',26,'#CAC9EE',spacing='3'))

write('youtube-thumbnail-template-1280x720',1280,720,background(1280,720)+
      text(80,95,'CHORO / GETTING STARTED',22,'#CAC9EE',spacing='2')+
      text(76,288,'Meet Choro.',86,weight='700')+
      text(80,368,'Your first workflow.',42)+
      text(81,628,'01',38,'#CAC9EE')+icon(935,250,240))
print('Exported six SVG layouts and six PNG assets.')
