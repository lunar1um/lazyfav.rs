# lazyfav.rs
<sub>an amazing tool to quickly add the currently playing spotify track into your favourite list!! (without having to open spotify :shock:)</sub>  

### why..?
because it's cool, we all love automation right?  
and uhh, the binary size is smaller than v1 (which is a great news)

### install
#### spotify api credentials
1. head to [spotify dev dashboard](https://developer.spotify.com/dashboard/), and login if you haven't  
2. fill in the required fields:  
- App name, description can be anything  
- Redirect URI: `http://127.0.0.1:8888/callback`  
- Choose `Web API` in the `Which API/SDKs are you planning to use?` section   
3. check the TOS box and **Save**  
4. once you got back to the **dashboard**, locate the previously created app. copy the **Client ID** and **Client Secret** (Hidden under **View client secret** button)  

#### configs
unlike the previous version, which requires locating and making a json file,  
this one just needs the credentials on **PATH**  
- `SPOTIFY_CLIENT_ID` = the **Client ID**
- `SPOTIFY_CLIENT_SECRET` = the **Client Secret**

if the script doesn't detect the corresponding **PATH**, it will notify!!

#### get the script (aka binary)
just go to **Releases** buddy  

#### okay now run
it will authorize on first time, after that everything is automated :3

#### also
i use this in my [dotfiles](https://github.com/lunar1um/dotfiles)  